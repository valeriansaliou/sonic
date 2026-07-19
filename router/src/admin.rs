// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::directory::{Directory, PlacementState};
use crate::error::{RouterError, RouterResult};
use crate::protocol::ChannelMode;
use crate::proxy::request_backend;
use crate::shutdown::Shutdown;

const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const ADMIN_BUFFER_SIZE: usize = 64 * 1024;
const ADMIN_THREAD_NAME: &str = "sonic-router-admin-client";
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

pub struct AdminServer {
    pub address: SocketAddr,
    pub auth_password: Option<String>,
    pub directory: Arc<Directory>,
    pub backend_timeout: Duration,
}

#[derive(Serialize)]
struct AdminResponse<T: Serialize> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

impl AdminServer {
    pub fn run(self, shutdown: Shutdown) -> RouterResult<()> {
        let listener = TcpListener::bind(self.address)?;
        listener.set_nonblocking(true)?;

        tracing::info!("router admin listening on tcp://{}", self.address);

        while !shutdown.is_requested() {
            match listener.accept() {
                Ok((stream, _)) => {
                    let directory = Arc::clone(&self.directory);
                    let password = self.auth_password.clone();
                    let timeout = self.backend_timeout;
                    thread::Builder::new()
                        .name(ADMIN_THREAD_NAME.to_owned())
                        .spawn(move || {
                            if let Err(error) = handle_client(stream, directory, password, timeout)
                            {
                                tracing::warn!("admin client disconnected: {error}");
                            }
                        })?;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(ACCEPT_POLL_INTERVAL);
                }
                Err(error) => tracing::warn!("admin connection failed: {error}"),
            }
        }

        tracing::info!("router admin stopped");

        Ok(())
    }
}

fn handle_client(
    stream: TcpStream,
    directory: Arc<Directory>,
    password: Option<String>,
    backend_timeout: Duration,
) -> RouterResult<()> {
    stream.set_read_timeout(Some(AUTH_TIMEOUT))?;
    stream.set_write_timeout(Some(AUTH_TIMEOUT))?;
    stream.set_nodelay(true)?;

    let reader_stream = stream.try_clone()?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    if let Some(password) = password {
        let auth = read_limited_line(&mut reader)?.ok_or_else(|| RouterError::code("closed"))?;

        if auth.trim_end() != format!("AUTH {password}") {
            write_error(&mut writer, "authentication_failed")?;
            return Ok(());
        }

        write_data(&mut writer, &"authenticated")?;
    }

    writer.set_read_timeout(Some(backend_timeout))?;
    writer.set_write_timeout(Some(backend_timeout))?;

    while let Some(line) = read_limited_line(&mut reader)? {
        let result = dispatch(
            &directory,
            line.trim_end_matches(['\r', '\n']),
            backend_timeout,
        );
        let written = match result {
            Ok(value) => write_data(&mut writer, &value),
            Err(error) => write_error(&mut writer, &error.to_string()),
        };

        written?;
    }

    Ok(())
}

fn dispatch(
    directory: &Directory,
    line: &str,
    backend_timeout: Duration,
) -> RouterResult<serde_json::Value> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        ["PLACEMENTS", id] => Ok(serde_json::to_value(directory.placements_for_backend(id)?)?),
        ["MIGRATE", "START", collection, bucket, target] => {
            to_json(directory.start_migration(collection, bucket, target)?)
        }
        ["MIGRATE", "CATCHUP", collection, bucket] => {
            to_json(directory.mark_catching_up(collection, bucket)?)
        }
        ["MIGRATE", "CUTOVER", collection, bucket] => {
            to_json(directory.cutover(collection, bucket)?)
        }
        ["MIGRATE", "DRAIN", collection, bucket] => {
            to_json(directory.mark_draining(collection, bucket)?)
        }
        ["MIGRATE", "CLEANUP", collection, bucket] => {
            let placement = directory.placement(collection, bucket)?;

            if placement.state != PlacementState::Draining {
                return Err(RouterError::code("migration_not_draining"));
            }

            let previous = placement
                .previous
                .ok_or_else(|| RouterError::code("previous_backend_missing"))?;
            let backend = directory
                .backends()?
                .remove(&previous)
                .ok_or_else(|| RouterError::code("previous_backend_missing"))?;

            let response = request_backend(
                ChannelMode::Ingest,
                &backend,
                &format!("FLUSHB {collection} {bucket}"),
                backend_timeout,
                ADMIN_BUFFER_SIZE,
            )?;

            if !response.starts_with("RESULT ") {
                return Err(RouterError::context("cleanup_failed", response));
            }

            to_json(directory.finish_migration(collection, bucket)?)
        }
        ["MIGRATE", "FINISH", collection, bucket] => {
            to_json(directory.finish_migration(collection, bucket)?)
        }
        ["MIGRATE", "ROLLBACK", collection, bucket] => {
            to_json(directory.rollback(collection, bucket)?)
        }
        ["SNAPSHOT"] => to_json(directory.snapshot()?),
        _ => Err(RouterError::code("unknown_admin_command")),
    }
}

fn to_json(value: impl Serialize) -> RouterResult<serde_json::Value> {
    Ok(serde_json::to_value(value)?)
}

fn write_data(writer: &mut TcpStream, data: &impl Serialize) -> std::io::Result<()> {
    let response = AdminResponse {
        ok: true,
        data: Some(data),
        error: None,
    };

    serde_json::to_writer(&mut *writer, &response)?;
    writer.write_all(b"\n")
}

fn write_error(writer: &mut TcpStream, error: &str) -> std::io::Result<()> {
    let response = AdminResponse::<()> {
        ok: false,
        data: None,
        error: Some(error.to_owned()),
    };

    serde_json::to_writer(&mut *writer, &response)?;
    writer.write_all(b"\n")
}

fn read_limited_line(reader: &mut impl BufRead) -> RouterResult<Option<String>> {
    let limit = ADMIN_BUFFER_SIZE + 2;
    let mut line = String::with_capacity(1024);
    let read = Read::by_ref(reader)
        .take(limit as u64)
        .read_line(&mut line)?;

    if read == 0 {
        return Ok(None);
    }

    if (read == limit && !line.ends_with('\n'))
        || line.trim_end_matches(['\r', '\n']).len() > ADMIN_BUFFER_SIZE
    {
        return Err(RouterError::code("admin_buffer_overflow"));
    }

    Ok(Some(line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendConfig;
    use crate::directory::BackendStatus;

    #[test]
    fn cleanup_flushes_previous_backend_and_finishes_migration() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();

        let backend_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"CONNECTED <mock-sonic>\r\n").unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            stream.write_all(b"STARTED ingest\r\n").unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "FLUSHB messages bucket");
            stream.write_all(b"RESULT 42\r\n").unwrap();
        });

        let temporary = tempfile::tempdir().unwrap();
        let source = backend_config("source", &address.to_string());
        let target = backend_config("target", "127.0.0.1:9");
        let directory = Directory::open(
            temporary.path().join("directory.db"),
            std::slice::from_ref(&source),
        )
        .unwrap();
        directory.assign("messages", "bucket").unwrap();
        directory.replace_backends(&[source, target]).unwrap();
        directory
            .start_migration("messages", "bucket", "target")
            .unwrap();
        directory.mark_catching_up("messages", "bucket").unwrap();
        directory.cutover("messages", "bucket").unwrap();
        directory.mark_draining("messages", "bucket").unwrap();

        dispatch(
            &directory,
            "MIGRATE CLEANUP messages bucket",
            Duration::from_secs(1),
        )
        .unwrap();

        backend_thread.join().unwrap();

        let placement = directory.placement("messages", "bucket").unwrap();
        assert_eq!(placement.state, PlacementState::Stable);
        assert!(placement.previous.is_none());
    }

    fn backend_config(id: &str, address: &str) -> BackendConfig {
        BackendConfig {
            id: id.to_owned(),
            address: address.to_owned(),
            auth_password: String::new(),
            status: BackendStatus::Active,
            weight: 1,
        }
    }
}
