// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, LittleEndian};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DOCUMENT_FORMAT_VERSION: u8 = 1;
const HEADER_SIZE: usize = 1 + 8 + 4 + 4;
pub(crate) const TIME_SLICE_MS: u64 = 60 * 60 * 1000;
pub const MAX_DOCUMENT_BYTES: usize = 14_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreDocument {
    pub oid: String,
    pub timestamp_ms: u64,
    pub text: String,
    pub metadata: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoreDocumentRecord {
    pub bucket: String,
    #[serde(flatten)]
    pub document: StoreDocument,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoreBulkResult {
    pub written: usize,
    pub rejected: usize,
}

pub(crate) struct StoreFreshBatchResult {
    pub written: usize,
    pub rejected: usize,
    pub frequencies: Vec<(String, u32)>,
    pub timings: StoreFreshBatchTimings,
    pub health: StoreKVHealth,
}

#[derive(Default)]
pub(crate) struct StoreFreshBatchTimings {
    pub metadata_reads: Duration,
    pub document_encode: Duration,
    pub oid_reads: Duration,
    pub term_reads: Duration,
    pub posting_reads: Duration,
    pub frequency_reads: Duration,
    pub time_posting_reads: Duration,
    pub batch_finalize: Duration,
    pub database_write: Duration,
    pub write_wal: Duration,
    pub write_memtable: Duration,
    pub write_delay: Duration,
    pub write_pre_and_post: Duration,
    pub write_db_mutex: Duration,
    pub write_db_condition_wait: Duration,
    pub write_merge_operator: Duration,
    pub batch_index_bytes: u64,
    pub batch_postings_bytes: u64,
    pub batch_documents_bytes: u64,
    pub batch_put_count: u64,
    pub batch_delete_count: u64,
    pub batch_merge_count: u64,
    pub oid_read_count: usize,
    pub term_read_count: usize,
    pub posting_read_count: usize,
    pub frequency_read_count: usize,
    pub time_posting_read_count: usize,
}

#[derive(Default)]
pub(crate) struct StoreKVHealth {
    pub index_l0_files: Option<u64>,
    pub postings_l0_files: Option<u64>,
    pub documents_l0_files: Option<u64>,
    pub index_pending_compaction_bytes: Option<u64>,
    pub postings_pending_compaction_bytes: Option<u64>,
    pub documents_pending_compaction_bytes: Option<u64>,
    pub delayed_write_rate: Option<u64>,
    pub write_stopped: Option<u64>,
}

impl StoreFreshBatchTimings {
    pub fn add_assign(&mut self, other: &Self) {
        self.metadata_reads += other.metadata_reads;
        self.document_encode += other.document_encode;
        self.oid_reads += other.oid_reads;
        self.term_reads += other.term_reads;
        self.posting_reads += other.posting_reads;
        self.frequency_reads += other.frequency_reads;
        self.time_posting_reads += other.time_posting_reads;
        self.batch_finalize += other.batch_finalize;
        self.database_write += other.database_write;
        self.write_wal += other.write_wal;
        self.write_memtable += other.write_memtable;
        self.write_delay += other.write_delay;
        self.write_pre_and_post += other.write_pre_and_post;
        self.write_db_mutex += other.write_db_mutex;
        self.write_db_condition_wait += other.write_db_condition_wait;
        self.write_merge_operator += other.write_merge_operator;
        self.batch_index_bytes += other.batch_index_bytes;
        self.batch_postings_bytes += other.batch_postings_bytes;
        self.batch_documents_bytes += other.batch_documents_bytes;
        self.batch_put_count += other.batch_put_count;
        self.batch_delete_count += other.batch_delete_count;
        self.batch_merge_count += other.batch_merge_count;
        self.oid_read_count += other.oid_read_count;
        self.term_read_count += other.term_read_count;
        self.posting_read_count += other.posting_read_count;
        self.frequency_read_count += other.frequency_read_count;
        self.time_posting_read_count += other.time_posting_read_count;
    }
}

impl StoreKVHealth {
    pub fn max_assign(&mut self, other: &Self) {
        self.index_l0_files = max_option(self.index_l0_files, other.index_l0_files);
        self.postings_l0_files = max_option(self.postings_l0_files, other.postings_l0_files);
        self.documents_l0_files = max_option(self.documents_l0_files, other.documents_l0_files);
        self.index_pending_compaction_bytes = max_option(
            self.index_pending_compaction_bytes,
            other.index_pending_compaction_bytes,
        );
        self.documents_pending_compaction_bytes = max_option(
            self.documents_pending_compaction_bytes,
            other.documents_pending_compaction_bytes,
        );
        self.postings_pending_compaction_bytes = max_option(
            self.postings_pending_compaction_bytes,
            other.postings_pending_compaction_bytes,
        );
        self.delayed_write_rate = max_option(self.delayed_write_rate, other.delayed_write_rate);
        self.write_stopped = max_option(self.write_stopped, other.write_stopped);
    }
}

fn max_option(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    }
}

impl StoreDocument {
    pub(crate) fn encoded_lengths(encoded: &[u8]) -> Result<(usize, usize), ()> {
        if encoded.len() < HEADER_SIZE || encoded[0] != DOCUMENT_FORMAT_VERSION {
            return Err(());
        }
        let text_len = LittleEndian::read_u32(&encoded[9..13]) as usize;
        let metadata_len = LittleEndian::read_u32(&encoded[13..17]) as usize;
        if HEADER_SIZE
            .checked_add(text_len)
            .and_then(|size| size.checked_add(metadata_len))
            != Some(encoded.len())
        {
            return Err(());
        }
        Ok((text_len, metadata_len))
    }

    pub fn new(
        oid: impl Into<String>,
        timestamp_ms: u64,
        text: impl Into<String>,
        metadata: serde_json::Value,
    ) -> Result<Self, ()> {
        if !metadata.is_object() {
            return Err(());
        }
        Ok(Self {
            oid: oid.into(),
            timestamp_ms,
            text: text.into(),
            metadata,
        })
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, ()> {
        let text = self.text.as_bytes();
        let metadata = serde_json::to_vec(&self.metadata).map_err(|_| ())?;
        let text_len = u32::try_from(text.len()).map_err(|_| ())?;
        let metadata_len = u32::try_from(metadata.len()).map_err(|_| ())?;
        let mut encoded = Vec::with_capacity(HEADER_SIZE + text.len() + metadata.len());
        encoded.push(DOCUMENT_FORMAT_VERSION);
        encoded.extend_from_slice(&self.timestamp_ms.to_le_bytes());
        encoded.extend_from_slice(&text_len.to_le_bytes());
        encoded.extend_from_slice(&metadata_len.to_le_bytes());
        encoded.extend_from_slice(text);
        encoded.extend_from_slice(&metadata);
        if encoded.len() > MAX_DOCUMENT_BYTES {
            return Err(());
        }
        Ok(encoded)
    }

    pub(crate) fn decode(oid: String, encoded: &[u8]) -> Result<Self, ()> {
        let (text_len, metadata_len) = Self::encoded_lengths(encoded)?;
        let timestamp_ms = LittleEndian::read_u64(&encoded[1..9]);
        let text_end = HEADER_SIZE.checked_add(text_len).ok_or(())?;
        let metadata_end = text_end.checked_add(metadata_len).ok_or(())?;
        if metadata_end != encoded.len() {
            return Err(());
        }
        let text = std::str::from_utf8(&encoded[HEADER_SIZE..text_end])
            .map_err(|_| ())?
            .to_owned();
        let metadata = serde_json::from_slice(&encoded[text_end..metadata_end]).map_err(|_| ())?;
        Self::new(oid, timestamp_ms, text, metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_round_trips_documents() {
        let document = StoreDocument::new(
            "message:1",
            1_721_297_600_123,
            "Hello world",
            serde_json::json!({"author": "alice"}),
        )
        .unwrap();
        assert_eq!(
            StoreDocument::decode(document.oid.clone(), &document.encode().unwrap()),
            Ok(document)
        );
    }

    #[test]
    fn it_rejects_non_object_metadata_and_invalid_payloads() {
        assert!(StoreDocument::new("message:1", 0, "", serde_json::json!([])).is_err());
        assert!(StoreDocument::decode("message:1".to_owned(), &[]).is_err());
        let oversized = StoreDocument::new(
            "message:1",
            0,
            "x".repeat(MAX_DOCUMENT_BYTES),
            serde_json::json!({}),
        )
        .unwrap();
        assert!(oversized.encode().is_err());
    }
}
