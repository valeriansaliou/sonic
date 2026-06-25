// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::{HashMap, VecDeque};

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
                pending: HashMap::with_capacity(8),
            },
        };

        (this, tx)
    }
}

pub(crate) struct Task<Discriminant> {
    pub command: Command,
    pub discriminant: Discriminant,
    pub callback: Box<dyn FnOnce(std::io::Result<(&str, &mut Tasks<Discriminant>)>) + Send>,
}

/// Just a struct that unties the lifetime of `SonicStream` from the rest in
/// `SonicConnection` (necessary when dealing with mutable references).
pub(crate) struct Tasks<Discriminant> {
    // TODO: Add a non-regression test to ensure the response ordering
    //   assumption is correct.
    /// Operations waiting for a response.
    ///
    /// Because we use a discriminant as key and a queue as value, we assume
    /// Sonic’s responses are ordered (e.g. `PENDING`s arrive in the same order
    /// as `QUERY` commands were sent). Although it’s not explicited in the
    /// protocol definition, the way Sonic is implemented enforces this to be
    /// true.
    pub(crate) pending:
        HashMap<Discriminant, VecDeque<Box<dyn FnOnce(&str, &mut Tasks<Discriminant>) + Send>>>,
}

impl<D: Discriminant> Tasks<D> {
    pub(crate) fn register_pending(
        &mut self,
        discriminant: D,
        callback: Box<dyn FnOnce(&str, &mut Tasks<D>) + Send>,
    ) {
        match self.pending.get_mut(&discriminant) {
            Some(pending) => pending.push_back(callback),
            None => {
                // If the discriminant is a unit variant, create the queue with
                // more capacity from the beginning.
                let queue_capacity: usize = if discriminant.has_payload() {
                    1
                } else {
                    COMMAND_QUEUE_SIZE
                };

                let mut pending = VecDeque::with_capacity(queue_capacity);

                pending.push_back(callback);

                self.pending.insert(discriminant, pending);
            }
        }
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

            let (discriminant, data) = match str::from_utf8(&line_bytes[..]) {
                Ok(line) => match (self.parse_line)(line) {
                    Ok(ok) => ok,
                    Err(error) => {
                        log_warn!("Invalid message received from the server: {error}");
                        continue 'process_lines;
                    }
                },

                Err(error) => {
                    log_warn!("Invalid UTF-8 sequence received from the server: {error}");
                    continue 'process_lines;
                }
            };

            let Some(pending) = self.tasks.pending.get_mut(&discriminant) else {
                log_warn!(
                    "Unexpected message received from the server: {discriminant:?} (expected: {:?})",
                    self.tasks.pending.keys()
                );
                continue 'process_lines;
            };

            let Some(respond) = pending.pop_front() else {
                log_warn!(
                    "Unexpected message received from the server: {discriminant:?} (queue empty)"
                );
                continue 'process_lines;
            };

            // NOTE: No need to clean up entries if the discriminant is
            //   a unit variant (e.g. `Ok`). It will be reused by other
            //   requests, saving a reallocation and keping the capacity
            //   around what’s needed.
            if discriminant.has_payload() && pending.is_empty() {
                self.tasks.pending.remove(&discriminant);
            }

            // log_trace!("Responding to {discriminant:?}");

            respond(data, &mut self.tasks);
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
            self.tasks.register_pending(
                task.discriminant,
                Box::new(move |str, dispatcher| (task.callback)(Ok((str, dispatcher)))),
            );
        }

        self.stream.flush_writes()
    }
}

impl<T: Transport, D> AsMut<mio::net::TcpStream> for SonicConnection<T, D> {
    fn as_mut(&mut self) -> &mut mio::net::TcpStream {
        self.stream.as_mut()
    }
}
