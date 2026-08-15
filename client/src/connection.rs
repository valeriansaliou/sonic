// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::VecDeque;

use crate::channel::Discriminant;
use crate::multiplexer::SonicConnectionTrait;
use crate::transport::Transport;
use crate::{COMMAND_QUEUE_SIZE, Command};

pub struct SonicConnection<T, Discriminant> {
    stream: T,

    parse_line: Box<dyn Fn(&str) -> std::io::Result<(Discriminant, &str)> + Send>,

    /// Message queue used for non-mutating writes to `write_buf`.
    task_rx: crossbeam_channel::Receiver<Task<Discriminant>>,

    tasks: Tasks<Discriminant>,
}

impl<T, D> SonicConnection<T, D> {
    pub fn new(
        stream: T,
        parse_line: impl Fn(&str) -> std::io::Result<(D, &str)> + Send + 'static,
    ) -> (Self, crossbeam_channel::Sender<Task<D>>) {
        let (tx, rx) = crossbeam_channel::bounded(COMMAND_QUEUE_SIZE);

        let this = Self {
            stream,
            parse_line: Box::new(parse_line),
            task_rx: rx,
            tasks: Tasks {
                pending: VecDeque::with_capacity(COMMAND_QUEUE_SIZE),
            },
        };

        (this, tx)
    }
}

pub(crate) type TaskCallback<Discriminant> =
    Box<dyn FnOnce(std::io::Result<(&str, &mut Tasks<Discriminant>)>) + Send>;

pub(crate) struct Task<Discriminant> {
    pub command: Command,
    pub discriminant: Discriminant,
    pub callback: TaskCallback<Discriminant>,
}

pub(crate) struct PendingTask<Discriminant> {
    pub discriminant: Discriminant,
    pub callback: TaskCallback<Discriminant>,
    pub is_user_initiated: bool,
}

/// Just a struct that unties the lifetime of `SonicStream` from the rest in
/// `SonicConnection` (necessary when dealing with mutable references).
pub(crate) struct Tasks<Discriminant> {
    // TODO: Add a non-regression test to ensure the response ordering
    //   assumption is correct.
    /// Operations waiting for a response.
    ///
    /// Because of Sonic Channel protocol limitations, we have to assume
    /// Sonic’s responses are ordered (e.g. `PENDING`s arrive in the same order
    /// as `QUERY` commands were sent). Although it’s not explicited in the
    /// protocol definition, the way Sonic is implemented enforces this to be
    /// true.
    pub(crate) pending: VecDeque<PendingTask<Discriminant>>,
}

impl<D: Discriminant> Tasks<D> {
    pub(crate) fn register_pending(&mut self, task: PendingTask<D>) {
        self.pending.push_back(task);
    }

    /// Removes and returns the first pending task satisfying the provided
    /// condition.
    fn pop_front(&mut self, cond: impl Fn(&PendingTask<D>) -> bool) -> Option<PendingTask<D>> {
        let first = self.pending.pop_front()?;

        // PERF: Check first entry before allocating another `VecDequeue` and
        //   doing the de-queue/re-queue logic, as the desired task is likely
        //   the first (answers mostly arrive in request order).
        if cond(&first) {
            return Some(first);
        }

        // TODO: Look for a smarter way to do this without allocating
        //   (e.g. smart swapping?).
        let mut dequeued: VecDeque<PendingTask<D>> =
            VecDeque::with_capacity(self.pending.len() + 1);
        dequeued.push_back(first);

        while let Some(pending) = self.pending.pop_front() {
            if cond(&pending) {
                // Re-queue dequeued tasks.
                while let Some(pending) = dequeued.pop_back() {
                    dequeued.push_front(pending);
                }

                return Some(pending);
            }

            dequeued.push_back(pending);
        }

        // `self.pending` is now empty, and `dequeued` is ordered so let’s just
        // replace the instance.
        assert!(self.pending.is_empty());
        self.pending = dequeued;

        None
    }
}

impl<T: Transport, D: Discriminant> SonicConnectionTrait for SonicConnection<T, D> {
    #[inline]
    fn wants_to_write(&self) -> bool {
        !self.task_rx.is_empty()
    }

    #[inline]
    fn wants_to_read(&self) -> bool {
        !self.tasks.pending.is_empty()
    }

    fn interest(&self) -> Option<mio::Interest> {
        match (self.wants_to_write(), self.wants_to_read()) {
            (false, false) => None,
            (true, false) => Some(mio::Interest::WRITABLE),
            (false, true) => Some(mio::Interest::READABLE),
            (true, true) => Some(mio::Interest::READABLE | mio::Interest::WRITABLE),
        }
    }

    /// Read incoming data (i.e. parse lines and dispatch responses).
    fn drain_reads(&mut self) -> std::io::Result<()> {
        'process_lines: for line_bytes in self.stream.read_lines()? {
            log_trace!("Read {} bytes line", line_bytes.len());

            let line = match str::from_utf8(&line_bytes[..]) {
                Ok(line) => line,
                Err(error) => {
                    log_warn!("Invalid UTF-8 sequence received from the server: {error}");
                    continue 'process_lines;
                }
            };

            if let Some(args) = line.strip_prefix("ERR ") {
                let Some(task) = self.tasks.pop_front(|task| task.is_user_initiated) else {
                    log_warn!(
                        ?line,
                        "Server sent an error but no user initiated task is pending."
                    );
                    continue 'process_lines;
                };

                (task.callback)(Err(std::io::Error::other(args)))
            }

            let (discriminant, data) = match (self.parse_line)(line) {
                Ok(ok) => ok,
                Err(error) => {
                    log_warn!(?line, "Invalid message received from the server: {error}");
                    continue 'process_lines;
                }
            };

            let Some(task) = (self.tasks).pop_front(|task| task.discriminant == discriminant)
            else {
                log_warn!(
                    "Unexpected message received from the server: {discriminant:?} (expected: {:?})",
                    (self.tasks.pending.iter())
                        .map(|task| &task.discriminant)
                        .collect::<Vec<_>>()
                );
                continue 'process_lines;
            };

            // log_trace!("Responding to {discriminant:?}");

            (task.callback)(Ok((data, &mut self.tasks)));
        }

        Ok(())
    }

    /// Write queued commands.
    fn flush_writes(&mut self) -> std::io::Result<usize> {
        // SAFETY: The limit prevents the loop from running foever if the
        //   channel gets filled from the inside.
        for task in self.task_rx.try_iter().take(self.task_rx.len()) {
            // Write command to the write buffer.
            self.stream.write_line(task.command);

            // Register pending task.
            self.tasks.register_pending(PendingTask {
                discriminant: task.discriminant,
                callback: task.callback,
                is_user_initiated: true,
            });
        }

        self.stream.flush_writes()
    }
}

impl<T: Transport, D> AsMut<mio::net::TcpStream> for SonicConnection<T, D> {
    fn as_mut(&mut self) -> &mut mio::net::TcpStream {
        self.stream.as_mut()
    }
}
