// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::sync::LazyLock;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROFILE_PATH_ENV: &str = "SONIC_INGEST_PROFILE_PATH";
const PROFILE_QUEUE_SIZE: usize = 1_024;
const PROFILE_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

static PROFILE_SENDER: LazyLock<Option<SyncSender<String>>> = LazyLock::new(open_profile_writer);

#[derive(Serialize)]
pub struct IngestCommandProfile {
    pub timestamp_ms: u128,
    pub payload_bytes: usize,
    pub compressed_bytes: usize,
    pub decoded_bytes: usize,
    pub command_total_us: u128,
    pub base64_decode_us: u128,
    pub decompress_us: u128,
    pub json_decode_us: u128,
    #[serde(flatten)]
    pub executor: sonic::executor::IngestProfile,
}

impl IngestCommandProfile {
    pub fn timestamp_ms() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }
}

pub fn enabled() -> bool {
    PROFILE_SENDER.is_some()
}

pub fn record(profile: &IngestCommandProfile) {
    let Some(sender) = PROFILE_SENDER.as_ref() else {
        return;
    };
    let Ok(encoded) = serde_json::to_string(profile) else {
        tracing::warn!("failed serializing ingest profile");
        return;
    };
    if let Err(error) = sender.try_send(encoded) {
        match error {
            TrySendError::Full(_) => {
                tracing::warn!("dropping ingest profile: writer queue is full")
            }
            TrySendError::Disconnected(_) => {
                tracing::warn!("dropping ingest profile: writer stopped")
            }
        }
    }
}

fn open_profile_writer() -> Option<SyncSender<String>> {
    let path = std::env::var_os(PROFILE_PATH_ENV)?;
    let file = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => file,
        Err(error) => {
            tracing::error!("failed opening ingest profile at {:?}: {}", path, error);
            return None;
        }
    };
    let (sender, receiver) = sync_channel::<String>(PROFILE_QUEUE_SIZE);
    if let Err(error) = thread::Builder::new()
        .name("sonic-ingest-profile".to_owned())
        .spawn(move || {
            let mut writer = BufWriter::new(file);
            loop {
                match receiver.recv_timeout(PROFILE_FLUSH_INTERVAL) {
                    Ok(line) => {
                        if writeln!(writer, "{line}").is_err() {
                            return;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if writer.flush().is_err() {
                            return;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        writer.flush().ok();
                        return;
                    }
                }
            }
        })
    {
        tracing::error!("failed spawning ingest profile writer: {}", error);
        return None;
    }
    Some(sender)
}
