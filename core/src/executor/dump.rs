// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::io::BufRead;
use std::path::Path;

use crate::lexer::{TokenLexerBuilder, TokenLexerMode};
use crate::store::StoreItemBuilder;
use crate::store::document::StoreDocumentRecord;
use crate::store::kv::StoreKVAcquireMode;

impl super::Executor {
    pub fn dump_bucket(
        &self,
        collection: &str,
        bucket: &str,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<StoreDocumentRecord>, ()> {
        let _guard = self.kv_pool.lock_read_access();
        let kv_store = self
            .kv_pool
            .acquire(StoreKVAcquireMode::OpenOnly, collection)?;
        executor_kv_lock_read!(kv_store);
        kv_store.as_ref().map_or(Ok(Vec::new()), |store| {
            store.dump_bucket(bucket, offset, limit)
        })
    }

    pub fn list_buckets(
        &self,
        collection: &str,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<String>, ()> {
        let _guard = self.kv_pool.lock_read_access();
        let kv_store = self
            .kv_pool
            .acquire(StoreKVAcquireMode::OpenOnly, collection)?;
        executor_kv_lock_read!(kv_store);
        kv_store
            .as_ref()
            .map_or(Ok(Vec::new()), |store| store.list_buckets(offset, limit))
    }

    pub fn export_documents(
        &self,
        collection: &str,
        bucket: Option<&str>,
        path: &Path,
    ) -> Result<u64, ()> {
        let _guard = self.kv_pool.lock_read_access();
        let store = self
            .kv_pool
            .acquire(StoreKVAcquireMode::OpenOnly, collection)?;
        executor_kv_lock_read!(store);
        store
            .as_ref()
            .map_or(Ok(0), |store| store.export_documents(bucket, path))
    }

    pub fn import_documents(&self, collection: &str, path: &Path) -> Result<u64, ()> {
        let file = std::fs::File::open(path).map_err(|_| ())?;
        let decoder = zstd::stream::read::Decoder::new(file).map_err(|_| ())?;
        let reader = std::io::BufReader::new(decoder);
        let mut count = 0;
        for line in reader.lines() {
            let record: StoreDocumentRecord =
                serde_json::from_str(&line.map_err(|_| ())?).map_err(|_| ())?;
            let document = record.document;
            let item = StoreItemBuilder::from_depth_3(collection, &record.bucket, &document.oid)
                .map_err(|_| ())?;
            let lexer = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                None,
                &document.text,
                self.app_conf.normalization,
                self.app_conf.tokenization,
            )?;
            self.upsert(item, lexer, document.clone())?;
            count += 1;
        }
        Ok(count)
    }
}
