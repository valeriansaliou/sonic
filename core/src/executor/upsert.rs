// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::lexer::{TokenLexer, TokenLexerBuilder, TokenLexerMode};
use crate::store::document::{
    StoreBulkResult, StoreDocument, StoreDocumentRecord, StoreFreshBatchTimings, StoreKVHealth,
};
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder};
use crate::store::{StoreItem, StoreItemBuilder};

#[derive(Serialize)]
pub struct IngestProfile {
    pub documents: usize,
    pub fresh: bool,
    pub executor_total_us: u128,
    pub tokenize_us: u128,
    pub kv_access_wait_us: u128,
    pub fst_access_wait_us: u128,
    pub kv_acquire_us: u128,
    pub kv_lock_wait_us: u128,
    pub kv_total_us: u128,
    pub kv_metadata_us: u128,
    pub document_encode_us: u128,
    pub oid_reads_us: u128,
    pub oid_read_count: usize,
    pub term_reads_us: u128,
    pub term_read_count: usize,
    pub posting_reads_us: u128,
    pub posting_read_count: usize,
    pub frequency_reads_us: u128,
    pub frequency_read_count: usize,
    pub time_posting_reads_us: u128,
    pub time_posting_read_count: usize,
    pub batch_finalize_us: u128,
    pub rocksdb_write_us: u128,
    pub write_wal_us: u128,
    pub write_memtable_us: u128,
    pub write_delay_us: u128,
    pub write_pre_and_post_us: u128,
    pub write_db_mutex_us: u128,
    pub write_db_condition_wait_us: u128,
    pub write_merge_operator_us: u128,
    pub batch_index_bytes: u64,
    pub batch_postings_bytes: u64,
    pub batch_documents_bytes: u64,
    pub batch_put_count: u64,
    pub batch_delete_count: u64,
    pub batch_merge_count: u64,
    pub kv_cpu_other_us: u128,
    pub fst_us: u128,
    pub l0_index: Option<u64>,
    pub l0_postings: Option<u64>,
    pub l0_documents: Option<u64>,
    pub pending_compaction_index_bytes: Option<u64>,
    pub pending_compaction_postings_bytes: Option<u64>,
    pub pending_compaction_documents_bytes: Option<u64>,
    pub delayed_write_rate: Option<u64>,
    pub write_stopped: Option<u64>,
    pub fst_pending_consolidations: usize,
}

impl super::Executor {
    pub fn upsert_batch(
        &self,
        collection: &str,
        records: Vec<StoreDocumentRecord>,
        fresh: bool,
    ) -> Result<StoreBulkResult, ()> {
        self.upsert_batch_inner(collection, records, fresh, false)
            .map(|(result, _)| result)
    }

    pub fn upsert_batch_profiled(
        &self,
        collection: &str,
        records: Vec<StoreDocumentRecord>,
        fresh: bool,
    ) -> Result<(StoreBulkResult, IngestProfile), ()> {
        self.upsert_batch_inner(collection, records, fresh, true)
    }

    fn upsert_batch_inner(
        &self,
        collection: &str,
        records: Vec<StoreDocumentRecord>,
        fresh: bool,
        profiling: bool,
    ) -> Result<(StoreBulkResult, IngestProfile), ()> {
        let total_started = Instant::now();
        let document_count = records.len();
        if !fresh {
            let mut result = StoreBulkResult::default();
            for record in records {
                let document = record.document;
                let item =
                    StoreItemBuilder::from_depth_3(collection, &record.bucket, &document.oid)
                        .map_err(|_| ())?;
                let lexer = TokenLexerBuilder::from(
                    TokenLexerMode::NormalizeAndCleanup,
                    None,
                    &document.text,
                    self.app_conf.normalization,
                    self.app_conf.tokenization,
                )?;
                match self.upsert(item, lexer, document.clone()) {
                    Ok(()) => result.written += 1,
                    Err(()) => result.rejected += 1,
                }
            }
            let mut profile = Self::build_upsert_batch_profile(
                document_count,
                false,
                total_started.elapsed(),
                Duration::ZERO,
                Duration::ZERO,
                Duration::ZERO,
                Duration::ZERO,
                Duration::ZERO,
                Duration::ZERO,
                Duration::ZERO,
                &StoreFreshBatchTimings::default(),
                &StoreKVHealth::default(),
            );
            profile.fst_pending_consolidations = self.fst_pool.count().1;
            return Ok((result, profile));
        }

        let tokenization_started = Instant::now();
        let mut buckets = HashMap::<String, Vec<(StoreDocument, Vec<String>)>>::new();
        for record in records {
            let document = record.document;
            let lexer = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                None,
                &document.text,
                self.app_conf.normalization,
                self.app_conf.tokenization,
            )?;
            let terms = lexer
                .map(|(token, _)| token.as_str().to_owned())
                .collect::<Vec<_>>();
            buckets
                .entry(record.bucket)
                .or_default()
                .push((document, terms));
        }
        let tokenization = tokenization_started.elapsed();

        let kv_access_wait_started = Instant::now();
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let kv_access_wait = kv_access_wait_started.elapsed();
        let fst_access_wait_started = Instant::now();
        let _fst_read_guard = self.fst_pool.lock_read_access();
        let fst_access_wait = fst_access_wait_started.elapsed();
        let kv_acquire_started = Instant::now();
        let kv_store = self.kv_pool.acquire(StoreKVAcquireMode::Any, collection)?;
        let kv_acquire = kv_acquire_started.elapsed();
        let kv_store_reference = kv_store.clone();
        let kv_store_lock_wait_started = Instant::now();
        let _kv_store_lock = kv_store_reference
            .as_ref()
            .map(|inner| inner.lock.write().unwrap());
        let kv_store_lock_wait = kv_store_lock_wait_started.elapsed();
        let mut result = StoreBulkResult::default();
        let mut kv_total = Duration::ZERO;
        let mut fst_total = Duration::ZERO;
        let mut kv_timings = StoreFreshBatchTimings::default();
        let mut kv_health = StoreKVHealth::default();
        for (bucket, documents) in buckets {
            let item = StoreItemBuilder::from_depth_2(collection, &bucket).map_err(|_| ())?;
            let StoreItem(_, Some(bucket_part), None) = item else {
                return Err(());
            };
            let kv_action = StoreKVActionBuilder::access_or_create(bucket_part, kv_store.clone());
            let bucket_id = kv_action.bucket_id().ok_or(())?;
            let kv_started = Instant::now();
            let batch_result = kv_action.batch_insert_fresh_documents(&documents, profiling)?;
            kv_total += kv_started.elapsed();
            result.written += batch_result.written;
            result.rejected += batch_result.rejected;
            kv_timings.add_assign(&batch_result.timings);
            kv_health.max_assign(&batch_result.health);
            let fst_started = Instant::now();
            let fst_store = self.fst_pool.acquire(collection, bucket_id)?;
            let fst_action = StoreFSTActionBuilder::access(fst_store);
            for (term, frequency) in batch_result.frequencies {
                fst_action.push_word(&term, frequency, &self.app_conf.store.fst);
            }
            fst_total += fst_started.elapsed();
        }
        let mut profile = Self::build_upsert_batch_profile(
            document_count,
            true,
            total_started.elapsed(),
            tokenization,
            kv_access_wait,
            fst_access_wait,
            kv_acquire,
            kv_store_lock_wait,
            kv_total,
            fst_total,
            &kv_timings,
            &kv_health,
        );
        profile.fst_pending_consolidations = self.fst_pool.count().1;
        Ok((result, profile))
    }

    #[allow(clippy::too_many_arguments)]
    fn build_upsert_batch_profile(
        documents: usize,
        fresh: bool,
        total: Duration,
        tokenization: Duration,
        kv_access_wait: Duration,
        fst_access_wait: Duration,
        kv_acquire: Duration,
        kv_store_lock_wait: Duration,
        kv_total: Duration,
        fst_total: Duration,
        kv: &StoreFreshBatchTimings,
        health: &StoreKVHealth,
    ) -> IngestProfile {
        let measured_kv = kv.metadata_reads
            + kv.document_encode
            + kv.oid_reads
            + kv.term_reads
            + kv.posting_reads
            + kv.frequency_reads
            + kv.time_posting_reads
            + kv.batch_finalize
            + kv.database_write;
        let kv_cpu_other = kv_total.saturating_sub(measured_kv);

        IngestProfile {
            documents,
            fresh,
            executor_total_us: total.as_micros(),
            tokenize_us: tokenization.as_micros(),
            kv_access_wait_us: kv_access_wait.as_micros(),
            fst_access_wait_us: fst_access_wait.as_micros(),
            kv_acquire_us: kv_acquire.as_micros(),
            kv_lock_wait_us: kv_store_lock_wait.as_micros(),
            kv_total_us: kv_total.as_micros(),
            kv_metadata_us: kv.metadata_reads.as_micros(),
            document_encode_us: kv.document_encode.as_micros(),
            oid_reads_us: kv.oid_reads.as_micros(),
            oid_read_count: kv.oid_read_count,
            term_reads_us: kv.term_reads.as_micros(),
            term_read_count: kv.term_read_count,
            posting_reads_us: kv.posting_reads.as_micros(),
            posting_read_count: kv.posting_read_count,
            frequency_reads_us: kv.frequency_reads.as_micros(),
            frequency_read_count: kv.frequency_read_count,
            time_posting_reads_us: kv.time_posting_reads.as_micros(),
            time_posting_read_count: kv.time_posting_read_count,
            batch_finalize_us: kv.batch_finalize.as_micros(),
            rocksdb_write_us: kv.database_write.as_micros(),
            write_wal_us: kv.write_wal.as_micros(),
            write_memtable_us: kv.write_memtable.as_micros(),
            write_delay_us: kv.write_delay.as_micros(),
            write_pre_and_post_us: kv.write_pre_and_post.as_micros(),
            write_db_mutex_us: kv.write_db_mutex.as_micros(),
            write_db_condition_wait_us: kv.write_db_condition_wait.as_micros(),
            write_merge_operator_us: kv.write_merge_operator.as_micros(),
            batch_index_bytes: kv.batch_index_bytes,
            batch_postings_bytes: kv.batch_postings_bytes,
            batch_documents_bytes: kv.batch_documents_bytes,
            batch_put_count: kv.batch_put_count,
            batch_delete_count: kv.batch_delete_count,
            batch_merge_count: kv.batch_merge_count,
            kv_cpu_other_us: kv_cpu_other.as_micros(),
            fst_us: fst_total.as_micros(),
            l0_index: health.index_l0_files,
            l0_postings: health.postings_l0_files,
            l0_documents: health.documents_l0_files,
            pending_compaction_index_bytes: health.index_pending_compaction_bytes,
            pending_compaction_postings_bytes: health.postings_pending_compaction_bytes,
            pending_compaction_documents_bytes: health.documents_pending_compaction_bytes,
            delayed_write_rate: health.delayed_write_rate,
            write_stopped: health.write_stopped,
            fst_pending_consolidations: 0,
        }
    }

    pub fn upsert(
        &self,
        item: StoreItem,
        lexer: TokenLexer,
        document: StoreDocument,
    ) -> Result<(), ()> {
        let StoreItem(collection, Some(bucket), Some(object)) = item else {
            return Err(());
        };
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();
        let kv_store = self.kv_pool.acquire(StoreKVAcquireMode::Any, collection)?;
        executor_kv_lock_write!(kv_store);
        let kv_action = StoreKVActionBuilder::access_or_create(bucket, kv_store);
        let bucket_id = kv_action.bucket_id().ok_or(())?;
        let fst_store = self.fst_pool.acquire(collection, bucket_id)?;
        let fst_action = StoreFSTActionBuilder::access(fst_store);
        let (iid, is_new_iid) = kv_action.resolve_or_reserve_iid(object.as_str())?;
        let old_terms = kv_action.get_iid_to_terms(iid)?.unwrap_or_default();
        let mut new_terms = Vec::new();
        for (token, _) in lexer {
            let term = token.as_str().to_owned();
            if !new_terms.contains(&term) {
                new_terms.push(term);
            }
        }

        let frequencies = kv_action.batch_upsert_document(
            iid,
            object.as_str(),
            is_new_iid,
            &old_terms,
            &new_terms,
            &document,
        )?;
        for (term, frequency) in frequencies {
            if frequency == 0 {
                fst_action.pop_word(&term);
            } else {
                fst_action.push_word(&term, frequency, &self.app_conf.store.fst);
            }
        }
        Ok(())
    }
}
