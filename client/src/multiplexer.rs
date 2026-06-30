// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::Arc;
use std::thread::JoinHandle;

use slab::Slab;

use crate::{MIO_EVENTS_CAPACITY, SEND_TIMEOUT};

pub(crate) enum MultiplexerTask {
    Attach(Box<dyn SonicConnectionTrait>),
}

pub struct SonicMultiplexer {
    _event_loop_handle: JoinHandle<std::io::Result<()>>,
    tx: crossbeam_channel::Sender<MultiplexerTask>,
    pub(crate) poll_waker: Arc<mio::Waker>,
}

impl SonicMultiplexer {
    /// Creates a new Sonic connection multiplexer.
    ///
    /// Invoking this functions spawns a single new thread which will act as
    /// an event loop for all connections created subsequently.
    ///
    /// # Errors
    ///
    /// Fails if the underlying `epoll`/`kqueue` creation fails.
    pub fn new() -> std::io::Result<Self> {
        let mut poll = mio::Poll::new()?;

        // TODO: Parameterize capacity?
        let (tx, rx) = crossbeam_channel::bounded(8);

        let poll_waker = Arc::new(mio::Waker::new(poll.registry(), mio::Token(usize::MAX))?);

        // TODO: Do not auto-start? So one can spawn the task differently or
        //   observe events (e.g. in tests)?
        let event_loop_handle = std::thread::spawn(move || run_event_loop(&mut poll, rx));

        Ok(Self {
            _event_loop_handle: event_loop_handle,
            tx,
            poll_waker,
        })
    }

    pub(crate) fn attach<C: SonicConnectionTrait + 'static>(&self, conn: C) -> std::io::Result<()> {
        if let Err(error) =
            (self.tx).send_timeout(MultiplexerTask::Attach(Box::new(conn)), SEND_TIMEOUT)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                error.to_string(),
            ));
        };
        self.poll_waker.wake()
    }
}

pub(crate) trait SonicConnectionTrait: AsMut<mio::net::TcpStream> + Send {
    fn wants_to_write(&self) -> bool;

    fn wants_to_read(&self) -> bool;

    fn interest(&self) -> Option<mio::Interest> {
        match (self.wants_to_write(), self.wants_to_read()) {
            (false, false) => None,
            (true, false) => Some(mio::Interest::WRITABLE),
            (false, true) => Some(mio::Interest::READABLE),
            (true, true) => Some(mio::Interest::READABLE | mio::Interest::WRITABLE),
        }
    }

    /// Read incoming data (i.e. parse lines and dispatch responses).
    fn drain_reads(&mut self) -> std::io::Result<()>;

    /// Write queued commands.
    fn flush_writes(&mut self) -> std::io::Result<usize>;
}

fn run_event_loop(
    poll: &mut mio::Poll,
    rx: crossbeam_channel::Receiver<MultiplexerTask>,
) -> std::io::Result<()> {
    // TODO: Parameterize capacity?
    let mut connections: Slab<ConnectionState> = Slab::with_capacity(1);

    // TODO: Parameterize capacity?
    // TODO: Auto-adjust capacity over time to optimize performances?
    let mut events = mio::Events::with_capacity(MIO_EVENTS_CAPACITY);

    loop {
        // SAFETY: We can use no timeout since we are using a `mio::waker` to
        //   wake the poll (i.e. send an event) when needed. This way, this
        //   loop’s logic is only ever executed when necessary. If we used a
        //   timeout, this loop would be executed every `timeout`, at least
        //   (wasteful when idle).
        if let Err(e) = poll.poll(&mut events, None) {
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }

        // SAFETY: The limit prevents the loop from running foever if the
        //   channel gets filled from the inside.
        for task in rx.try_iter().take(rx.len()) {
            match task {
                MultiplexerTask::Attach(conn) => {
                    connections.insert(ConnectionState::new(conn));
                }
            }
        }

        for event in &events {
            log_trace!("Got `mio` event: {event:?}");

            let token = event.token();
            if let Some(conn) = connections.get_mut(token.0) {
                if event.is_readable() {
                    log_trace!("Draining read");
                    conn.drain_reads()?;
                }

                if event.is_writable() {
                    flush_writes(conn.as_mut())?;
                }

                // Remove the connection if dead.
                if event.is_error() || event.is_read_closed() {
                    log_debug!("Detaching connection {}", token.0);
                    connections.remove(token.0);
                }
            }
        }

        for (key, conn) in connections.iter_mut() {
            conn.update_interest(poll.registry(), mio::Token(key))?;

            // TODO: Check if this might be necessary (race condition).
            // log_trace!("Draining read after re-register");
            // conn.drain_reads()?;
        }
    }
}

fn flush_writes(conn: &mut dyn SonicConnectionTrait) -> std::io::Result<()> {
    match conn.flush_writes() {
        // If everything was written successfully, go back to read only
        // (until a new message is queued).
        Ok(n) => {
            log_debug!("Wrote {n} bytes (flush total)");
            Ok(())
        }

        // If the TCP stream would block, stay in read & write to continue
        // writing in next event loop cycle.
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),

        // Otherwise, bubble up the error.
        Err(error) => Err(error),
    }
}

struct ConnectionState {
    inner: Box<dyn SonicConnectionTrait>,
    interest: Option<mio::Interest>,
}

impl ConnectionState {
    fn new(inner: Box<dyn SonicConnectionTrait>) -> Self {
        Self {
            inner,
            interest: None,
        }
    }

    fn update_interest(
        &mut self,
        registry: &mio::Registry,
        token: mio::Token,
    ) -> std::io::Result<()> {
        let mut new_interest: Option<mio::Interest> = self.inner.interest();

        if new_interest != self.interest {
            let key = token.0;

            match (self.interest, new_interest) {
                (None, Some(new_interest)) => {
                    log_debug!("Registering #{key} for {new_interest:?}");
                    registry.register((*self.inner).as_mut(), token, new_interest)
                }

                (Some(_), Some(new_interest)) => {
                    log_debug!("Re-registering #{key} for {new_interest:?}");
                    registry.reregister((*self.inner).as_mut(), token, new_interest)
                }

                (Some(_), None) => {
                    log_debug!("De-registering #{key}");
                    registry.deregister((*self.inner).as_mut())
                }

                (None, None) => unreachable!("new_interest != self.interest"),
            }?;

            std::mem::swap(&mut self.interest, &mut new_interest);
        }

        Ok(())
    }
}

impl std::ops::Deref for ConnectionState {
    type Target = Box<dyn SonicConnectionTrait>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for ConnectionState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
