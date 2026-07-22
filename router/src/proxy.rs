// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::directory::{Backend, BackendStatus, Directory};
use crate::error::{RouterError, RouterResult};
use crate::protocol::{BatchCommand, ChannelMode, RoutedCommand, classify};
use crate::shutdown::Shutdown;

const PROTOCOL_REVISION: u8 = 3;
const ORDINARY_BUFFER_SIZE: usize = 20_000;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CLIENT_THREAD_NAME: &str = "sonic-router-client";
const BACKEND_THREAD_NAME: &str = "sonic-router-backend";
const CONNECTED_BANNER: &str = concat!(
    "CONNECTED <",
    env!("CARGO_PKG_NAME"),
    " v",
    env!("CARGO_PKG_VERSION"),
    ">"
);

pub struct ProxyServer {
    pub address: SocketAddr,
    pub auth_password: Option<String>,
    pub tcp_timeout: u64,
    pub bulk_buffer_size: usize,
    pub directory: Arc<Directory>,
}

struct BackendSession {
    writer: Mutex<TcpStream>,
}

struct SessionPool {
    sessions: HashMap<String, BackendSession>,
    client_writer: Arc<Mutex<TcpStream>>,
    mode: ChannelMode,
    timeout: Duration,
}

// Dedicated persistent connections for `UPSERTBATCH`: unlike `SessionPool` (which fans out \
//   every backend response straight to the client via a background thread), batch commands \
//   need a synchronous request/response on the same connection so their `RESULT` can be \
//   parsed and aggregated across backends before replying once to the client. Reusing one \
//   connection per backend across many batches avoids paying a fresh TCP connect + Sonic \
//   handshake (CONNECTED/START/STARTED, ie. 2 extra round trips) on every single batch.
struct BatchSession {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
}

struct BatchSessionPool {
    sessions: HashMap<String, BatchSession>,
    mode: ChannelMode,
    timeout: Duration,
}

impl BatchSessionPool {
    fn request(
        &mut self,
        backend: &Backend,
        command: &str,
        max_response: usize,
    ) -> RouterResult<String> {
        if !self.sessions.contains_key(&backend.id) {
            let session = connect_batch_session(backend, self.mode, self.timeout)?;
            self.sessions.insert(backend.id.clone(), session);
        }

        match Self::try_request(self.sessions.get_mut(&backend.id), command, max_response) {
            Ok(response) => Ok(response),
            // The cached connection may have gone stale (backend restart, idle timeout, ...); \
            //   drop it and retry exactly once against a freshly (re)connected session.
            Err(_) => {
                self.sessions.remove(&backend.id);
                let session = connect_batch_session(backend, self.mode, self.timeout)?;
                self.sessions.insert(backend.id.clone(), session);
                Self::try_request(self.sessions.get_mut(&backend.id), command, max_response)
            }
        }
    }

    fn try_request(
        session: Option<&mut BatchSession>,
        command: &str,
        max_response: usize,
    ) -> RouterResult<String> {
        let session = session.ok_or_else(|| RouterError::code("batch_session_missing"))?;
        write_line(&mut session.stream, command)?;
        let response = read_limited_line(&mut session.reader, max_response)?
            .ok_or_else(|| RouterError::code("backend_closed"))?;
        Ok(response.trim_end_matches(['\r', '\n']).to_owned())
    }
}

fn connect_batch_session(
    backend: &Backend,
    mode: ChannelMode,
    timeout: Duration,
) -> RouterResult<BatchSession> {
    let mut stream = connect_backend(&backend.address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_nodelay(true)?;

    let reader_stream = stream.try_clone()?;
    let mut reader = BufReader::new(reader_stream);

    read_backend_line(&mut reader, "CONNECTED")?;
    write_line(
        &mut stream,
        &format!("START {} {}", mode.as_str(), backend.auth_password),
    )?;
    read_backend_line(&mut reader, "STARTED")?;

    Ok(BatchSession { stream, reader })
}

impl ProxyServer {
    pub fn run(self, shutdown: Shutdown) -> RouterResult<()> {
        let listener = TcpListener::bind(self.address)?;
        listener.set_nonblocking(true)?;

        tracing::info!("router channel listening on tcp://{}", self.address);

        while !shutdown.is_requested() {
            match listener.accept() {
                Ok((stream, _)) => {
                    let directory = Arc::clone(&self.directory);
                    let password = self.auth_password.clone();
                    let timeout = self.tcp_timeout;
                    let bulk_buffer_size = self.bulk_buffer_size;
                    thread::Builder::new()
                        .name(CLIENT_THREAD_NAME.to_owned())
                        .spawn(move || {
                            if let Err(error) = handle_client(
                                stream,
                                directory,
                                password.as_deref(),
                                timeout,
                                bulk_buffer_size,
                            ) {
                                tracing::warn!("router client disconnected: {error}");
                            }
                        })?;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(ACCEPT_POLL_INTERVAL);
                }
                Err(error) => tracing::warn!("router connection failed: {error}"),
            }
        }

        tracing::info!("router channel stopped");

        Ok(())
    }
}

fn handle_client(
    mut stream: TcpStream,
    directory: Arc<Directory>,
    auth_password: Option<&str>,
    tcp_timeout: u64,
    bulk_buffer_size: usize,
) -> RouterResult<()> {
    configure_stream(&stream, HANDSHAKE_TIMEOUT)?;
    write_line(&mut stream, CONNECTED_BANNER)?;

    let reader_stream = stream.try_clone()?;
    let mut reader = BufReader::new(reader_stream);

    let start = read_limited_line(&mut reader, ORDINARY_BUFFER_SIZE)?
        .ok_or_else(|| RouterError::code("closed"))?;
    let mode = parse_start(start.trim_end(), auth_password)?;

    configure_stream(&stream, Duration::from_secs(tcp_timeout))?;
    write_line(
        &mut stream,
        &format!(
            "STARTED {} protocol({PROTOCOL_REVISION}) buffer({ORDINARY_BUFFER_SIZE}) bulk_buffer({bulk_buffer_size})",
            mode.as_str()
        ),
    )?;

    let client_writer = Arc::new(Mutex::new(stream));
    let mut sessions = SessionPool {
        sessions: HashMap::new(),
        client_writer: Arc::clone(&client_writer),
        mode,
        timeout: Duration::from_secs(tcp_timeout),
    };
    let mut batch_sessions = BatchSessionPool {
        sessions: HashMap::new(),
        mode,
        timeout: Duration::from_secs(tcp_timeout),
    };

    loop {
        let Some(line) = read_limited_line(&mut reader, bulk_buffer_size)? else {
            return Ok(());
        };

        let line = line.trim_end_matches(['\r', '\n']);
        let maximum = if line.starts_with("UPSERTBATCH ") {
            bulk_buffer_size
        } else {
            ORDINARY_BUFFER_SIZE
        };
        if line.len() > maximum {
            return Err(RouterError::code("command_buffer_overflow"));
        }

        match classify(mode, line) {
            RoutedCommand::Local(response) => {
                if !response.is_empty() {
                    write_shared(&client_writer, response)?;
                }
                if response == "ENDED quit" {
                    return Ok(());
                }
            }
            RoutedCommand::Reject(reason) => {
                write_shared(&client_writer, &format!("ERR policy_reject({reason})"))?;
            }
            RoutedCommand::Broadcast => {
                let backends = directory.backends()?;
                let mut error = None;
                let mut total: Option<u64> = None;

                for backend in backends
                    .values()
                    .filter(|backend| backend.status != BackendStatus::Offline)
                {
                    // Use the bulk buffer here: unlike `FLUSHC`/`COUNT` (tiny numeric results), \
                    //   list-shaped broadcasts (eg. `BUCKETS`) can return a large payload per \
                    //   backend, well past the ordinary command buffer.
                    match request_backend(mode, backend, line, sessions.timeout, bulk_buffer_size) {
                        Ok(response) if response == "OK" => {}
                        Ok(response) => match parse_broadcast_result(&response) {
                            Some(count) => total = Some(total.unwrap_or(0) + count),
                            None => {
                                error = Some(format!("{}:{response}", backend.id));
                                break;
                            }
                        },
                        Err(reason) => {
                            error = Some(format!("{}:{reason}", backend.id));
                            break;
                        }
                    }
                }

                match error {
                    Some(error) => {
                        write_shared(&client_writer, &format!("ERR broadcast_failed({error})"))?
                    }
                    None => match total {
                        Some(total) => write_shared(&client_writer, &format!("RESULT {total}"))?,
                        None => write_shared(&client_writer, "OK")?,
                    },
                }
            }
            RoutedCommand::BroadcastList => {
                let backends = directory.backends()?;
                let mut error = None;
                // De-duplicated: a bucket briefly exists on two backends while its migration \
                //   mirror is copying, so a naive concatenation could list it twice.
                let mut names = BTreeSet::<String>::new();

                for backend in backends
                    .values()
                    .filter(|backend| backend.status != BackendStatus::Offline)
                {
                    match request_backend(mode, backend, line, sessions.timeout, bulk_buffer_size) {
                        Ok(response) => match parse_broadcast_list(&response) {
                            Some(tokens) => names.extend(tokens),
                            None => {
                                error = Some(format!("{}:{response}", backend.id));
                                break;
                            }
                        },
                        Err(reason) => {
                            error = Some(format!("{}:{reason}", backend.id));
                            break;
                        }
                    }
                }

                match error {
                    Some(error) => {
                        write_shared(&client_writer, &format!("ERR broadcast_failed({error})"))?
                    }
                    None if names.is_empty() => write_shared(&client_writer, "RESULT")?,
                    None => write_shared(
                        &client_writer,
                        &format!("RESULT {}", names.into_iter().collect::<Vec<_>>().join(" ")),
                    )?,
                }
            }
            RoutedCommand::Bucket {
                collection,
                bucket,
                writing,
            } => {
                let route = match directory.route(&collection, &bucket, true, writing) {
                    Ok(route) => route,
                    Err(error) => {
                        write_shared(&client_writer, &format!("ERR internal_error({error})"))?;
                        continue;
                    }
                };

                if let Some(mirror) = route.mirror {
                    match request_backend(
                        mode,
                        &mirror,
                        line,
                        sessions.timeout,
                        ORDINARY_BUFFER_SIZE,
                    ) {
                        Ok(response) if !response.starts_with("ERR ") => {}
                        Ok(response) => {
                            write_shared(
                                &client_writer,
                                &format!("ERR mirror_rejected({response})"),
                            )?;
                            continue;
                        }
                        Err(error) => {
                            write_shared(
                                &client_writer,
                                &format!("ERR mirror_unavailable({error})"),
                            )?;
                            continue;
                        }
                    }
                }

                if let Err(error) = sessions.send(&route.primary, line) {
                    write_shared(&client_writer, &format!("ERR backend_unavailable({error})"))?;
                }
            }
            RoutedCommand::Batch(batch) => {
                match execute_batch(&directory, &batch, &mut batch_sessions) {
                    Ok((written, rejected)) => {
                        write_shared(&client_writer, &format!("RESULT {written} {rejected}"))?;
                    }
                    Err(error) => {
                        write_shared(&client_writer, &format!("ERR batch_failed({error})"))?;
                    }
                }
            }
        }
    }
}

impl SessionPool {
    fn send(&mut self, backend: &Backend, command: &str) -> RouterResult<()> {
        if !self.sessions.contains_key(&backend.id) {
            let session = connect_session(
                backend,
                self.mode,
                self.timeout,
                Arc::clone(&self.client_writer),
            )?;
            self.sessions.insert(backend.id.clone(), session);
        }

        let session = self
            .sessions
            .get(&backend.id)
            .ok_or_else(|| RouterError::code("backend_session_missing"))?;
        let mut writer = session
            .writer
            .lock()
            .map_err(|_| RouterError::code("backend_session_poisoned"))?;
        write_line(&mut writer, command)
    }
}

fn connect_session(
    backend: &Backend,
    mode: ChannelMode,
    timeout: Duration,
    client_writer: Arc<Mutex<TcpStream>>,
) -> RouterResult<BackendSession> {
    let mut stream = connect_backend(&backend.address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_nodelay(true)?;

    let reader_stream = stream.try_clone()?;
    let mut reader = BufReader::new(reader_stream);

    read_backend_line(&mut reader, "CONNECTED")?;
    write_line(
        &mut stream,
        &format!("START {} {}", mode.as_str(), backend.auth_password),
    )?;
    read_backend_line(&mut reader, "STARTED")?;

    thread::Builder::new()
        .name(BACKEND_THREAD_NAME.to_owned())
        .spawn(move || {
            for response in reader.lines() {
                let Ok(response) = response else {
                    break;
                };
                if write_shared(&client_writer, response.trim_end_matches('\r')).is_err() {
                    break;
                }
            }
        })?;

    Ok(BackendSession {
        writer: Mutex::new(stream),
    })
}

fn execute_batch(
    directory: &Directory,
    batch: &BatchCommand,
    batch_sessions: &mut BatchSessionPool,
) -> RouterResult<(u64, u64)> {
    let mut primary_records = BTreeMap::<String, Vec<_>>::new();
    let mut mirror_records = BTreeMap::<String, Vec<_>>::new();
    let mut backends = BTreeMap::<String, Backend>::new();

    for record in &batch.records {
        let route = directory.route(&batch.collection, &record.bucket, true, true)?;
        backends.insert(route.primary.id.clone(), route.primary.clone());
        primary_records
            .entry(route.primary.id)
            .or_default()
            .push(record);
        if let Some(mirror) = route.mirror {
            backends.insert(mirror.id.clone(), mirror.clone());
            mirror_records.entry(mirror.id).or_default().push(record);
        }
    }

    for (backend_id, command) in batch.encode_groups(mirror_records)? {
        let backend = backends
            .get(&backend_id)
            .ok_or_else(|| RouterError::code("mirror_backend_missing"))?;
        let response = batch_sessions.request(backend, &command, ORDINARY_BUFFER_SIZE)?;
        parse_batch_result(&response)?;
    }

    let mut written = 0;
    let mut rejected = 0;

    for (backend_id, command) in batch.encode_groups(primary_records)? {
        let backend = backends
            .get(&backend_id)
            .ok_or_else(|| RouterError::code("primary_backend_missing"))?;
        let response = batch_sessions.request(backend, &command, ORDINARY_BUFFER_SIZE)?;
        let result = parse_batch_result(&response)?;
        written += result.0;
        rejected += result.1;
    }

    Ok((written, rejected))
}

pub fn request_backend(
    mode: ChannelMode,
    backend: &Backend,
    command: &str,
    timeout: Duration,
    max_response: usize,
) -> RouterResult<String> {
    let mut stream = connect_backend(&backend.address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let mut reader = BufReader::new(stream.try_clone()?);

    read_backend_line(&mut reader, "CONNECTED")?;
    write_line(
        &mut stream,
        &format!("START {} {}", mode.as_str(), backend.auth_password),
    )?;
    read_backend_line(&mut reader, "STARTED")?;

    write_line(&mut stream, command)?;

    let response = read_limited_line(&mut reader, max_response)?
        .ok_or_else(|| RouterError::code("backend_closed"))?;

    Ok(response.trim_end_matches(['\r', '\n']).to_owned())
}

fn parse_batch_result(response: &str) -> RouterResult<(u64, u64)> {
    let parts = response.split_whitespace().collect::<Vec<_>>();
    let ["RESULT", written, rejected] = parts.as_slice() else {
        return Err(RouterError::context("unexpected_batch_response", response));
    };

    Ok((
        written
            .parse()
            .map_err(|_| RouterError::code("invalid_written_count"))?,
        rejected
            .parse()
            .map_err(|_| RouterError::code("invalid_rejected_count"))?,
    ))
}

fn parse_broadcast_result(response: &str) -> Option<u64> {
    let parts = response.split_whitespace().collect::<Vec<_>>();
    let ["RESULT", count] = parts.as_slice() else {
        return None;
    };
    count.parse().ok()
}

fn parse_broadcast_list(response: &str) -> Option<Vec<String>> {
    let mut parts = response.split_whitespace();
    if parts.next()? != "RESULT" {
        return None;
    }
    Some(parts.map(str::to_owned).collect())
}

fn parse_start(line: &str, expected_password: Option<&str>) -> RouterResult<ChannelMode> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    match (parts.as_slice(), expected_password) {
        (["START", mode], None) => mode.parse(),
        (["START", mode, _], None) => mode.parse(),
        (["START", mode, password], Some(expected)) if *password == expected => mode.parse(),
        (["START", _, _], Some(_)) => Err(RouterError::code("authentication_failed")),
        _ => Err(RouterError::code("not_recognized")),
    }
}

fn configure_stream(stream: &TcpStream, timeout: Duration) -> RouterResult<()> {
    let timeout = Some(timeout);
    // On some platforms, a socket accepted from a non-blocking listener (used here to poll \
    //   `accept()` without a dedicated thread) inherits the non-blocking flag; without clearing \
    //   it, read/write timeouts below are ignored and calls fail immediately with EAGAIN instead \
    //   of blocking up to the timeout.
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    Ok(())
}

fn connect_backend(address: &str, timeout: Duration) -> RouterResult<TcpStream> {
    let addresses = address.to_socket_addrs()?;
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }

    match last_error {
        Some(error) => Err(error.into()),
        None => Err(RouterError::code("backend_address_resolved_empty")),
    }
}

fn read_backend_line(reader: &mut BufReader<TcpStream>, prefix: &str) -> RouterResult<String> {
    let line = read_limited_line(reader, ORDINARY_BUFFER_SIZE)?
        .ok_or_else(|| RouterError::code("backend_closed"))?;

    if !line.starts_with(prefix) {
        return Err(RouterError::context(
            "backend_handshake_failed",
            line.trim_end(),
        ));
    }

    Ok(line)
}

fn write_shared(writer: &Arc<Mutex<TcpStream>>, line: &str) -> RouterResult<()> {
    let mut writer = writer
        .lock()
        .map_err(|_| RouterError::code("client_writer_poisoned"))?;
    write_line(&mut writer, line)
}

fn write_line(writer: &mut TcpStream, line: &str) -> RouterResult<()> {
    writer.write_all(format!("{line}\r\n").as_bytes())?;
    Ok(())
}

fn read_limited_line(reader: &mut impl BufRead, maximum: usize) -> RouterResult<Option<String>> {
    let limit = maximum.saturating_add(3);
    let mut line = String::with_capacity(maximum.min(1024));
    let read = Read::by_ref(reader)
        .take(limit as u64)
        .read_line(&mut line)?;

    if read == 0 {
        return Ok(None);
    }

    let content = line.strip_suffix('\n').unwrap_or(&line);
    let content = content.strip_suffix('\r').unwrap_or(content);

    if content.len() > maximum || (read == limit && !line.ends_with('\n')) {
        return Err(RouterError::code("command_buffer_overflow"));
    }

    Ok(Some(line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendConfig;
    use crate::directory::{BackendStatus, Directory};
    use std::io::Cursor;
    use std::sync::mpsc;

    #[test]
    fn parses_authenticated_start() {
        assert_eq!(
            parse_start("START search secret", Some("secret")).unwrap(),
            ChannelMode::Search
        );
        assert!(parse_start("START search wrong", Some("secret")).is_err());
    }

    #[test]
    fn parses_batch_result_counts() {
        assert_eq!(parse_batch_result("RESULT 12 3").unwrap(), (12, 3));
        assert!(parse_batch_result("ERR query_error").is_err());
    }

    #[test]
    fn bounded_reader_rejects_oversized_commands() {
        let mut accepted = Cursor::new(b"12345\r\n");
        assert_eq!(
            read_limited_line(&mut accepted, 5).unwrap().unwrap(),
            "12345\r\n"
        );
        let mut rejected = Cursor::new(b"123456\r\n");
        assert_eq!(
            read_limited_line(&mut rejected, 5).unwrap_err().to_string(),
            "command_buffer_overflow"
        );
    }

    #[test]
    fn proxies_bucket_command_and_persists_placement() {
        let backend_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let backend_address = backend_listener.local_addr().unwrap();
        let backend_thread = thread::spawn(move || {
            let (mut stream, _) = backend_listener.accept().unwrap();
            write_line(&mut stream, "CONNECTED <mock-sonic>").unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "START ingest secret");
            write_line(&mut stream, "STARTED ingest").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("PUSH messages bucket:1 object:1"));
            write_line(&mut stream, "OK").unwrap();
        });

        let temporary = tempfile::tempdir().unwrap();
        let directory = Arc::new(
            Directory::open(
                temporary.path().join("directory.db"),
                &[test_backend("sonic-0", backend_address)],
            )
            .unwrap(),
        );

        let router_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let router_address = router_listener.local_addr().unwrap();
        let router_directory = Arc::clone(&directory);
        let router_thread = thread::spawn(move || {
            let (stream, _) = router_listener.accept().unwrap();
            handle_client(stream, router_directory, Some("router"), 5, 8_388_608).unwrap();
        });

        let mut stream = TcpStream::connect(router_address).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("CONNECTED"));
        write_line(&mut stream, "START ingest router").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("STARTED ingest"));
        write_line(
            &mut stream,
            r#"PUSH messages bucket:1 object:1 "hello world""#,
        )
        .unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "OK");
        write_line(&mut stream, "QUIT").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "ENDED quit");
        drop(stream);

        router_thread.join().unwrap();
        backend_thread.join().unwrap();
        assert_eq!(
            directory
                .route("messages", "bucket:1", false, false)
                .unwrap()
                .primary
                .id,
            "sonic-0"
        );
    }

    #[test]
    fn mirrors_writes_while_migration_is_copying() {
        let (source_address, source_rx, source_thread) = spawn_ingest_backend();
        let (target_address, target_rx, target_thread) = spawn_ingest_backend();
        let temporary = tempfile::tempdir().unwrap();
        let source = test_backend("source", source_address);
        let target = test_backend("target", target_address);
        let directory = Arc::new(
            Directory::open(
                temporary.path().join("directory.db"),
                std::slice::from_ref(&source),
            )
            .unwrap(),
        );
        directory.assign("messages", "bucket:1").unwrap();
        directory.replace_backends(&[source, target]).unwrap();
        directory
            .start_migration("messages", "bucket:1", "target")
            .unwrap();

        let router_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let router_address = router_listener.local_addr().unwrap();
        let router_directory = Arc::clone(&directory);
        let router_thread = thread::spawn(move || {
            let (stream, _) = router_listener.accept().unwrap();
            handle_client(stream, router_directory, None, 5, 8_388_608).unwrap();
        });
        let mut stream = TcpStream::connect(router_address).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        write_line(&mut stream, "START ingest").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        write_line(
            &mut stream,
            r#"PUSH messages bucket:1 object:1 "hello world""#,
        )
        .unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "OK");
        write_line(&mut stream, "QUIT").unwrap();
        drop(stream);

        router_thread.join().unwrap();
        source_thread.join().unwrap();
        target_thread.join().unwrap();
        assert!(
            source_rx
                .recv()
                .unwrap()
                .starts_with("PUSH messages bucket:1")
        );
        assert!(
            target_rx
                .recv()
                .unwrap()
                .starts_with("PUSH messages bucket:1")
        );
    }

    #[test]
    fn broadcasts_backup_trigger_to_online_backends() {
        let backend_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let backend_address = backend_listener.local_addr().unwrap();
        let backend_thread = thread::spawn(move || {
            let (mut stream, _) = backend_listener.accept().unwrap();
            write_line(&mut stream, "CONNECTED <mock-sonic>").unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "START control secret");
            write_line(&mut stream, "STARTED control").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "TRIGGER backup /tmp/sonic");
            write_line(&mut stream, "OK").unwrap();
        });
        let temporary = tempfile::tempdir().unwrap();
        let directory = Arc::new(
            Directory::open(
                temporary.path().join("directory.db"),
                &[test_backend("sonic-0", backend_address)],
            )
            .unwrap(),
        );
        let router_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let router_address = router_listener.local_addr().unwrap();
        let router_directory = Arc::clone(&directory);
        let router_thread = thread::spawn(move || {
            let (stream, _) = router_listener.accept().unwrap();
            handle_client(stream, router_directory, None, 5, 8_388_608).unwrap();
        });
        let mut stream = TcpStream::connect(router_address).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        write_line(&mut stream, "START control").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        write_line(&mut stream, "TRIGGER backup /tmp/sonic").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "OK");
        write_line(&mut stream, "QUIT").unwrap();
        drop(stream);
        router_thread.join().unwrap();
        backend_thread.join().unwrap();
    }

    #[test]
    fn broadcasts_flushc_and_sums_result_counts_across_backends() {
        let first = spawn_flushc_backend(30);
        let second = spawn_flushc_backend(12);
        let temporary = tempfile::tempdir().unwrap();
        let directory = Arc::new(
            Directory::open(
                temporary.path().join("directory.db"),
                &[
                    test_backend("sonic-0", first.0),
                    test_backend("sonic-1", second.0),
                ],
            )
            .unwrap(),
        );
        let router_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let router_address = router_listener.local_addr().unwrap();
        let router_directory = Arc::clone(&directory);
        let router_thread = thread::spawn(move || {
            let (stream, _) = router_listener.accept().unwrap();
            handle_client(stream, router_directory, Some("router"), 5, 8_388_608).unwrap();
        });
        let mut stream = TcpStream::connect(router_address).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        write_line(&mut stream, "START ingest router").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        write_line(&mut stream, "FLUSHC messages").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "RESULT 42");
        write_line(&mut stream, "QUIT").unwrap();
        drop(stream);
        router_thread.join().unwrap();
        first.1.join().unwrap();
        second.1.join().unwrap();
    }

    fn spawn_flushc_backend(result: u64) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_line(&mut stream, "CONNECTED <mock-sonic>").unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("START ingest"));
            write_line(&mut stream, "STARTED ingest").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "FLUSHC messages");
            write_line(&mut stream, &format!("RESULT {result}")).unwrap();
        });
        (address, handle)
    }

    #[test]
    fn broadcasts_buckets_and_deduplicates_names_across_backends() {
        let first = spawn_buckets_backend(&["bucket:1", "bucket:2"]);
        let second = spawn_buckets_backend(&["bucket:2", "bucket:3"]);
        let temporary = tempfile::tempdir().unwrap();
        let directory = Arc::new(
            Directory::open(
                temporary.path().join("directory.db"),
                &[
                    test_backend("sonic-0", first.0),
                    test_backend("sonic-1", second.0),
                ],
            )
            .unwrap(),
        );
        let router_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let router_address = router_listener.local_addr().unwrap();
        let router_directory = Arc::clone(&directory);
        let router_thread = thread::spawn(move || {
            let (stream, _) = router_listener.accept().unwrap();
            handle_client(stream, router_directory, Some("router"), 5, 8_388_608).unwrap();
        });
        let mut stream = TcpStream::connect(router_address).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        write_line(&mut stream, "START ingest router").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        write_line(&mut stream, "BUCKETS messages").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        let mut names = line.trim().split_whitespace().collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(names, ["RESULT", "bucket:1", "bucket:2", "bucket:3"]);
        write_line(&mut stream, "QUIT").unwrap();
        drop(stream);
        router_thread.join().unwrap();
        first.1.join().unwrap();
        second.1.join().unwrap();
    }

    fn spawn_buckets_backend(
        names: &'static [&'static str],
    ) -> (SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_line(&mut stream, "CONNECTED <mock-sonic>").unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("START ingest"));
            write_line(&mut stream, "STARTED ingest").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "BUCKETS messages");
            write_line(&mut stream, &format!("RESULT {}", names.join(" "))).unwrap();
        });
        (address, handle)
    }

    #[test]
    fn reuses_persistent_connection_across_batches() {
        let backend_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let backend_address = backend_listener.local_addr().unwrap();
        let (handshake_tx, handshake_rx) = mpsc::channel();
        let backend_thread = thread::spawn(move || {
            let (mut stream, _) = backend_listener.accept().unwrap();
            write_line(&mut stream, "CONNECTED <mock-sonic>").unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "START ingest secret");
            handshake_tx.send(()).unwrap();
            write_line(&mut stream, "STARTED ingest").unwrap();

            // Two UPSERTBATCH commands must arrive on this SAME accepted connection: if the \
            //   router reconnected per batch, `backend_listener.accept()` above would need to \
            //   be called again, which it never is in this test.
            for _ in 0..2 {
                line.clear();
                reader.read_line(&mut line).unwrap();
                assert!(line.starts_with("UPSERTBATCH messages upsert "));
                write_line(&mut stream, "RESULT 1 0").unwrap();
            }
        });

        let temporary = tempfile::tempdir().unwrap();
        let directory = Arc::new(
            Directory::open(
                temporary.path().join("directory.db"),
                &[test_backend("sonic-0", backend_address)],
            )
            .unwrap(),
        );

        let router_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let router_address = router_listener.local_addr().unwrap();
        let router_directory = Arc::clone(&directory);
        let router_thread = thread::spawn(move || {
            let (stream, _) = router_listener.accept().unwrap();
            handle_client(stream, router_directory, Some("router"), 5, 8_388_608).unwrap();
        });

        let mut stream = TcpStream::connect(router_address).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        write_line(&mut stream, "START ingest router").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();

        let upsertbatch = |oid: &str| -> String {
            use base64::Engine;
            let ndjson = format!(r#"{{"bucket":"bucket:1","oid":"{oid}"}}"#);
            let compressed = zstd::stream::encode_all(ndjson.as_bytes(), 1).unwrap();
            let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed);
            format!("UPSERTBATCH messages upsert {payload}")
        };

        for oid in ["a", "b"] {
            write_line(&mut stream, &upsertbatch(oid)).unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "RESULT 1 0");
        }

        write_line(&mut stream, "QUIT").unwrap();
        drop(stream);
        router_thread.join().unwrap();
        backend_thread.join().unwrap();

        // Exactly one handshake happened across both batches.
        assert_eq!(handshake_rx.try_iter().count(), 1);
    }

    fn test_backend(id: &str, address: SocketAddr) -> BackendConfig {
        BackendConfig {
            id: id.to_owned(),
            address: address.to_string(),
            auth_password: "secret".to_owned(),
            status: BackendStatus::Active,
            weight: 1,
        }
    }

    fn spawn_ingest_backend() -> (SocketAddr, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_line(&mut stream, "CONNECTED <mock-sonic>").unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "START ingest secret");
            write_line(&mut stream, "STARTED ingest").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            tx.send(line.trim().to_owned()).unwrap();
            write_line(&mut stream, "OK").unwrap();
        });
        (address, rx, handle)
    }
}
