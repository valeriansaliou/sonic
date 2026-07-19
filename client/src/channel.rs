// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use crate::SEND_TIMEOUT;
use crate::SonicMultiplexer;
use crate::connection::Task;
use crate::connection::{self, SonicConnection};
use crate::events::{self, ChannelInfo, ServerInfo};
use crate::transport::Transport;
use crate::util::errors::io_error_invalid_data;

pub trait Discriminant: std::fmt::Debug + Clone + Eq + std::hash::Hash + Send + 'static {
    /// Whether or not the discriminant has a payload (e.g. `Pending(_)`).
    fn has_payload(&self) -> bool;
}

pub trait ChannelMode {
    type Discriminant: Discriminant;

    fn name() -> &'static str;

    fn parse<'a>(
        discriminant: &'a str,
        rest: &'a str,
    ) -> std::io::Result<(Self::Discriminant, &'a str)>;

    fn parse_line(line: &str) -> std::io::Result<(Self::Discriminant, &str)> {
        // log_trace!("Parsing {line:?}");

        let (discriminant, rest) = match line.split_once(' ') {
            Some((discriminant, rest)) => (discriminant, rest),
            None if line.is_empty() => {
                return Err(io_error_invalid_data("Line missing discriminant"));
            }
            None => (line, &line[line.len()..]),
        };

        Self::parse(discriminant, rest)
    }
}

/// Lower level shared logic for interacting with Sonic through via the
/// Sonic Channel protocol.
///
/// This library could have exposed all functions through this single `struct`
/// but for better discoverability, better docs and separation of concern it
/// exposes wrappers (e.g. `SonicChannelIngestSync`, `SonicChannelSearchAsync`,
/// etc.).
pub struct SonicChannel<Mode: ChannelMode> {
    pub server_info: ServerInfo,
    pub channel_info: ChannelInfo,
    dispatcher_tx: crossbeam_channel::Sender<connection::Task<Mode::Discriminant>>,
    poll_waker: Arc<mio::Waker>,
    is_closed: bool,
}

impl<Mode: ChannelMode + 'static> SonicChannel<Mode> {
    #[doc(alias = "new")]
    pub fn connect<T: Transport + 'static>(
        addr: impl Into<std::net::SocketAddr>,
        pass: impl AsRef<str>,
        multiplexer: &SonicMultiplexer,
    ) -> std::io::Result<Self> {
        let mut stream = T::connect(addr.into())?;

        let server_info = {
            let response = stream.read_line_sync()?;

            events::Connected::from_str(&response)?.server_info
        };

        let channel_info = {
            stream.write_with(|buf| {
                use bytes::BufMut as _;
                buf.put_slice(b"START ");
                buf.put_slice(Mode::name().as_bytes());
                buf.put_slice(b" ");
                buf.put_slice(pass.as_ref().as_bytes());
            });
            stream.flush_writes().unwrap();

            let response = stream.read_line_sync()?;

            let Some(stripped) = response.strip_prefix(&format!("STARTED {} ", Mode::name()))
            else {
                return Err(io_error_invalid_data(response));
            };

            ChannelInfo::from_str(stripped).map_err(io_error_invalid_data)?
        };

        let (conn, tx) = SonicConnection::new(stream, Mode::parse_line);
        multiplexer.attach(conn)?;

        Ok(Self {
            server_info,
            channel_info,
            dispatcher_tx: tx,
            poll_waker: Arc::clone(&multiplexer.poll_waker),
            is_closed: false,
        })
    }

    pub(crate) fn send<T: Send + 'static>(
        &self,
        command: Command,
        discriminant: Mode::Discriminant,
        parse: impl Fn(&str) -> std::io::Result<T> + Send + 'static,
    ) -> std::io::Result<oneshot::Receiver<std::io::Result<T>>> {
        if command.len() > self.channel_info.buffer_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Command too long. Max buffer size: {}",
                    self.channel_info.buffer_size
                ),
            ));
        }

        let (tx, rx) = oneshot::channel();

        self.dispatcher_tx
            .send_timeout(
                Task {
                    command,
                    discriminant,
                    callback: Box::new(move |result| {
                        // log_debug!("Callback");

                        match result {
                            Ok((data, _)) => {
                                let send_res = tx.send(parse(data));

                                if let Err(error) = send_res {
                                    // Only log an error, as this would happen if the receiver is dropped.
                                    log_error!("Could not send response: {error}");
                                }
                            }

                            Err(error) => {
                                if let Err(send_error) = tx.send(Err(error)) {
                                    // Only log an error, as this would happen if the receiver is dropped.
                                    log_error!("Could not send response: {send_error}");
                                }
                            }
                        }
                    }),
                },
                SEND_TIMEOUT,
            )
            .map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, error.to_string())
            })?;
        self.poll_waker.wake()?;

        Ok(rx)
    }

    pub(crate) fn send_bulk<T: Send + 'static>(
        &self,
        command: Command,
        discriminant: Mode::Discriminant,
        parse: impl Fn(&str) -> std::io::Result<T> + Send + 'static,
    ) -> std::io::Result<oneshot::Receiver<std::io::Result<T>>> {
        if command.len() > self.channel_info.bulk_buffer_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Bulk command too long. Max bulk buffer size: {}",
                    self.channel_info.bulk_buffer_size
                ),
            ));
        }
        let (tx, rx) = oneshot::channel();
        self.dispatcher_tx
            .send_timeout(
                Task {
                    command,
                    discriminant,
                    callback: Box::new(move |result| match result {
                        Ok((data, _)) => {
                            let _ = tx.send(parse(data));
                        }
                        Err(error) => {
                            let _ = tx.send(Err(error));
                        }
                    }),
                },
                SEND_TIMEOUT,
            )
            .map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, error.to_string())
            })?;
        self.poll_waker.wake()?;
        Ok(rx)
    }

    /// Sends a command asynchronous at the protocol level (e.g. using the
    /// `PENDING` + `EVENT` pattern).
    pub(crate) fn send_async<T: Send + 'static>(
        &self,
        command: Command,
        discriminant1: impl Into<Mode::Discriminant>,
        make_discriminant2: impl FnOnce(&str) -> Mode::Discriminant + Send + Sync + 'static,
        parse: impl Fn(&str) -> std::io::Result<T> + Send + 'static,
    ) -> std::io::Result<oneshot::Receiver<std::io::Result<T>>> {
        if command.len() > self.channel_info.buffer_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Command too long. Max buffer size: {}",
                    self.channel_info.buffer_size
                ),
            ));
        }

        let (tx, rx) = oneshot::channel();

        self.dispatcher_tx
            .send_timeout(
                Task {
                    command,
                    discriminant: discriminant1.into(),
                    callback: Box::new(move |result| {
                        match result {
                            Ok((data, dispatcher)) => {
                                // NOTE: This will create for example `EventQuery("Bt2m2gYa")`.
                                let discriminant2 = make_discriminant2(data);

                                debug_assert!(!dispatcher.pending.contains_key(&discriminant2));

                                // Register pending operation (receiving the final result).
                                dispatcher.register_pending(
                                    discriminant2,
                                    Box::new(move |data, _| {
                                        let send_res = tx.send(parse(data));

                                        if let Err(error) = send_res {
                                            // Only log an error, as this would happen if the receiver is dropped.
                                            log_error!("Could not send response: {error}");
                                        }
                                    }),
                                );
                            }

                            Err(error) => {
                                if let Err(send_error) = tx.send(Err(error)) {
                                    // Only log an error, as this would happen if the receiver is dropped.
                                    log_error!("Could not send response: {send_error}");
                                }
                            }
                        }
                    }),
                },
                SEND_TIMEOUT,
            )
            .map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, error.to_string())
            })?;
        self.poll_waker.wake()?;

        Ok(rx)
    }

    pub(crate) fn send_async_stream<T: Default + Send + 'static>(
        &self,
        command: Command,
        discriminant1: impl Into<Mode::Discriminant>,
        make_discriminant2: impl FnOnce(&str) -> Mode::Discriminant + Send + Sync + 'static,
        reduce: impl Fn(&mut T, &str) -> std::io::Result<bool> + Send + Sync + 'static,
    ) -> std::io::Result<oneshot::Receiver<std::io::Result<T>>> {
        if command.len() > self.channel_info.buffer_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Command too long. Max buffer size: {}",
                    self.channel_info.buffer_size
                ),
            ));
        }
        let (tx, rx) = oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(tx)));
        let accumulator = Arc::new(Mutex::new(Some(T::default())));
        let reduce: Arc<dyn Fn(&mut T, &str) -> std::io::Result<bool> + Send + Sync> =
            Arc::new(reduce);
        self.dispatcher_tx
            .send_timeout(
                Task {
                    command,
                    discriminant: discriminant1.into(),
                    callback: Box::new(move |result| match result {
                        Ok((data, dispatcher)) => {
                            let discriminant = make_discriminant2(data);
                            Self::register_stream_response(
                                dispatcher,
                                discriminant,
                                Arc::clone(&sender),
                                Arc::clone(&accumulator),
                                Arc::clone(&reduce),
                            );
                        }
                        Err(error) => {
                            if let Some(tx) = sender.lock().unwrap().take() {
                                let _ = tx.send(Err(error));
                            }
                        }
                    }),
                },
                SEND_TIMEOUT,
            )
            .map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, error.to_string())
            })?;
        self.poll_waker.wake()?;
        Ok(rx)
    }

    fn register_stream_response<T: Default + Send + 'static>(
        dispatcher: &mut connection::Tasks<Mode::Discriminant>,
        discriminant: Mode::Discriminant,
        sender: Arc<Mutex<Option<oneshot::Sender<std::io::Result<T>>>>>,
        accumulator: Arc<Mutex<Option<T>>>,
        reduce: Arc<dyn Fn(&mut T, &str) -> std::io::Result<bool> + Send + Sync>,
    ) {
        let next_discriminant = discriminant.clone();
        dispatcher.register_pending(
            discriminant,
            Box::new(move |data, dispatcher| {
                let result = {
                    let mut guard = accumulator.lock().unwrap();
                    let value = guard.as_mut().expect("stream accumulator missing");
                    reduce(value, data)
                };
                match result {
                    Ok(true) => {
                        if let (Some(tx), Some(value)) = (
                            sender.lock().unwrap().take(),
                            accumulator.lock().unwrap().take(),
                        ) {
                            let _ = tx.send(Ok(value));
                        }
                    }
                    Ok(false) => Self::register_stream_response(
                        dispatcher,
                        next_discriminant,
                        sender,
                        accumulator,
                        reduce,
                    ),
                    Err(error) => {
                        if let Some(tx) = sender.lock().unwrap().take() {
                            let _ = tx.send(Err(error));
                        }
                    }
                }
            }),
        );
    }

    /// Sends a command as multiple commands if it’s too long.
    pub(crate) fn send_buffered<T: Default + Send + 'static>(
        &self,
        command: Command,
        discriminant: Mode::Discriminant,
        reduce: impl Fn(T, &str) -> std::io::Result<T> + Clone + Send + Sync + 'static,
    ) -> std::io::Result<oneshot::Receiver<std::io::Result<T>>> {
        if command.len() <= self.channel_info.buffer_size {
            return self.send(command, discriminant, move |data| {
                reduce(T::default(), data)
            });
        }

        let splits = command.split(self.channel_info.buffer_size);

        let mut final_rx: Option<oneshot::Receiver<std::io::Result<T>>> = None;

        for command in splits {
            let (tx, rx) = oneshot::channel::<std::io::Result<T>>();

            let previous_rx = final_rx.replace(rx);
            let reduce = reduce.clone();

            let task = Task {
                command,
                discriminant: discriminant.clone(),
                callback: Box::new(move |result| {
                    // NOTE: By referencing `previous_rx`, we keep the receiver
                    //   alive long enough so the channel is still open when a
                    //   result is sent, preventing misleading errors from
                    //   being logged.
                    let accumulator = match previous_rx {
                        Some(previous_rx) => {
                            let previous_res = previous_rx
                                // NOTE: We can have a very short timeout here
                                //   as a value is guaranteed to have already
                                //   arrived (by construction).
                                .recv_timeout(std::time::Duration::from_millis(10))
                                .unwrap_or_else(|error| {
                                    Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, error))
                                });
                            match previous_res {
                                Ok(t) => t,
                                err @ Err(_) => {
                                    if let Err(send_error) = tx.send(err) {
                                        // Only log an error, as this would happen if the receiver is dropped.
                                        log_error!("Could not send response: {send_error}");
                                    }
                                    return;
                                }
                            }
                        }
                        None => T::default(),
                    };

                    match result {
                        Ok((data, _tasks)) => {
                            let res = reduce(accumulator, data);
                            let send_res = tx.send(res);

                            if let Err(error) = send_res {
                                // Only log an error, as this would happen if the receiver is dropped.
                                log_error!("Could not send response: {error}");
                            }
                        }

                        Err(error) => {
                            if let Err(send_error) = tx.send(Err(error)) {
                                // Only log an error, as this would happen if the receiver is dropped.
                                log_error!("Could not send response: {send_error}");
                            }
                        }
                    }
                }),
            };

            self.dispatcher_tx
                .send_timeout(task, SEND_TIMEOUT)
                .map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, error.to_string())
                })?;
        }

        self.poll_waker.wake()?;

        // NOTE: Intermediate receivers will not be waited for, and potential
        //   errors will be discarded. However, not doing so would imply
        //   waiting for full roundtrips before sending the next chunk, which
        //   is very wasteful. Instead, we assume that a failing intermediate
        //   chunk would cause all chunks to fail (e.g. bad syntax). Also,
        //   because Sonic responses come in the same order as requests, we
        //   can be sure all messages have been processed by the time the last
        //   event is received.
        // SAFETY: We know for sure there is at least one chunk.
        Ok(final_rx.unwrap())
    }

    pub(crate) fn mark_closed(&mut self) {
        self.is_closed = true;
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.is_closed
    }
}

pub(crate) struct Command {
    value: Box<str>,
    prefix_len: usize,
    suffix_len: usize,
}

impl Command {
    pub fn new(value: Box<str>, prefix_len: usize, suffix_len: usize) -> Self {
        Self {
            value,
            prefix_len,
            suffix_len,
        }
    }

    /// Length, in bytes (not chars).
    pub fn len(&self) -> usize {
        self.value.len()
    }

    pub fn split(self, buffer_size: usize) -> Vec<Self> {
        let command_overhead = self.prefix_len + self.suffix_len;

        let mut content = &self.value[self.prefix_len..(self.value.len() - self.suffix_len)];

        let chunk_size = buffer_size - command_overhead;

        // NOTE: `+1` to account for backtracks (to split on spaces).
        let mut splits: Vec<Self> = Vec::with_capacity((content.len() / chunk_size) + 1);

        log_debug!(
            "Splitting command (content length: {content_len}, chunk size: {chunk_size}).",
            content_len = content.len()
        );

        while !content.is_empty() {
            let (chunk, rest) = split_on_whitespace(content, chunk_size);
            splits.push(self.with_content(chunk));
            content = rest;
        }

        log_debug!("Split command into {} chunks.", splits.len());

        splits
    }

    fn with_content(&self, content: &str) -> Self {
        let mut value = String::with_capacity(self.prefix_len + content.len() + self.suffix_len);

        value.push_str(&self.value[..self.prefix_len]);
        value.push_str(content);
        value.push_str(&self.value[(self.value.len() - self.suffix_len)..]);

        Self {
            value: value.into_boxed_str(),
            prefix_len: self.prefix_len,
            suffix_len: self.suffix_len,
        }
    }
}

impl From<&str> for Command {
    fn from(value: &str) -> Self {
        Self {
            prefix_len: value.len(),
            suffix_len: 0,
            value: Box::from(value),
        }
    }
}

impl AsRef<[u8]> for Command {
    fn as_ref(&self) -> &[u8] {
        self.value.as_bytes()
    }
}

impl std::fmt::Debug for Command {
    /// This debug formatter prints:
    ///
    /// - `TRIGGER consolidate` -> `TRIGGER consolidate`
    /// - `SEARCH c b "term"` -> `SEARCH c b "term"`
    /// - `PUSH c b o "long sentence"` -> `PUSH c b o "long…[5 bytes]…ence"`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.prefix_len + self.suffix_len < self.value.len() {
            // Print prefix.
            f.write_str(&self.value[..self.prefix_len])?;

            const CONTEXT_SIZE: usize = 10;

            let content = &self.value[self.prefix_len..(self.value.len() - self.suffix_len)];
            if content.len() < 2 * CONTEXT_SIZE + 2 {
                // Print whole command (short content).
                f.write_str(content)?;
            } else {
                // Split long content and add ellipses.
                f.write_str(&content[..CONTEXT_SIZE])?;
                write!(f, "…[{} bytes]…", (content.len() - CONTEXT_SIZE * 2))?;
                f.write_str(&content[(content.len() - CONTEXT_SIZE)..])?;
            }

            // Print suffix.
            f.write_str(&self.value[(self.value.len() - self.suffix_len)..])
        } else {
            // Print whole command (nothing to split on).
            f.write_str(&self.value)
        }
    }
}

/// Splits a string on the last whitespace before `n` if it’s larger
/// than `n`. If the string contains no whitespace, splits on last UTF-8
/// character boundary.
fn split_on_whitespace(s: &str, n: usize) -> (&str, &str) {
    if s.len() <= n {
        return (s, &s[s.len()..]);
    }

    // Make sure we slice at a char boundary.
    let end = s.ceil_char_boundary(n);

    // Search backward for the last whitespace.
    if let Some(i) = (s[..end].char_indices())
        .rev()
        .find_map(|(i, ch)| ch.is_ascii_whitespace().then_some(i))
    {
        // Split before the whitespace.
        let (a, b) = s.split_at(i);

        // Skip whitespace in second slice.
        (a, b.trim_ascii_start())
    } else {
        // Fallback to UTF-8 boundary if no whitespace exists.
        // SAFETY: We can safely unwrap here, as `s` cannot be empty.
        let i = s[..end].char_indices().last().unwrap().0;

        s.split_at(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_on_whitespace() {
        assert_eq!(split_on_whitespace("foo bar ", 10), ("foo bar ", ""));

        assert_eq!(
            split_on_whitespace("foobar foobar ", 10),
            ("foobar", "foobar ")
        );

        assert_eq!(split_on_whitespace("foo", 3), ("foo", ""));

        // NOTE: This isn’t ideal, but it would add branching to fix this case,
        //   which we honestly don’t care about.
        assert_eq!(split_on_whitespace("foobar", 3), ("fo", "obar"));

        assert_eq!(split_on_whitespace("cinéma", 4), ("cin", "éma"));

        // NOTE: This isn’t ideal, but it would add branching to fix this case,
        //   which we honestly don’t care about.
        assert_eq!(split_on_whitespace("cinéma", 5), ("cin", "éma"));

        // There used to be a bug caused by slicing at `1`:
        //
        // ```log
        // start byte index 1 is not a char boundary; it is inside '\u{a0}' (bytes 0..2) of ` km². `
        // ```
        //
        // This is a non-regression test.
        // NOTE: We usually don’t split on non-breaking spaces, and look for an
        //   ASCII space only, but if we don’t find an ASCII space we split on
        //   whatever UTF-8 boundary we find.
        assert_eq!(split_on_whitespace("300 km². ", 4), ("300", " km². "));
    }
}
