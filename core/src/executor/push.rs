// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use linked_hash_set::LinkedHashSet;
use rocksdb::WriteBatch;
use std::iter::FromIterator;
use std::sync::Arc;

use crate::lexer::TokenLexer;
use crate::store::StoreItem;
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::identifiers::StoreTermHashed;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder};
use crate::util::itertools::ExactSizeIteratorExt as _;

impl super::Executor {
    pub fn push(&self, item: StoreItem, lexer: TokenLexer) -> Result<(), ()> {
        let StoreItem(collection, Some(bucket), Some(object)) = item else {
            return Err(());
        };

        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let (Ok(kv_store), Ok(fst_store)) = (
            self.kv_pool.acquire(StoreKVAcquireMode::Any, collection),
            self.fst_pool.acquire(collection, bucket),
        ) else {
            return Err(());
        };

        debug_assert!(kv_store.is_some());
        let Some(kv_store) = kv_store else {
            tracing::error!(
                "collection store {collection:?} does not exist, but it should have been created"
            );
            return Err(());
        };

        let (kv_action, fst_action) = (
            StoreKVActionBuilder::access_read_write(bucket, Arc::clone(&kv_store)),
            StoreFSTActionBuilder::access(fst_store),
        );

        // Try to resolve existing OID to IID, otherwise initialize IID (store the \
        //   bi-directional relationship)
        let oid = object.as_str();
        let write_guard = kv_store.lock.write().unwrap();
        let iid = kv_action.get_oid_to_iid(oid).unwrap_or(None).or_else(|| {
            tracing::info!("must initialize push executor oid-to-iid and iid-to-oid");

            // Bump last stored increment
            match kv_action.auto_increment_iid(Some(write_guard)) {
                Ok(iid) => {
                    let mut batch = WriteBatch::default();

                    // Associate OID <> IID (bidirectional)
                    kv_action.set_oid_to_iid(&mut batch, oid, iid);
                    kv_action.set_iid_to_oid(&mut batch, iid, oid);

                    executor_ensure_op!(kv_action.write(batch));

                    Some(iid)
                }
                Err(error) => {
                    tracing::error!("{error}");

                    None
                }
            }
        });

        let Some(iid) = iid else {
            return Err(());
        };

        let mut has_commits = false;

        // Acquire list of terms for IID
        let mut iid_terms_hashed: LinkedHashSet<StoreTermHashed> = LinkedHashSet::from_iter(
            kv_action
                .get_iid_to_terms(iid)
                .unwrap_or(None)
                .unwrap_or_default(),
        );

        tracing::debug!(
            "got push executor stored iid-to-terms: {:?}",
            iid_terms_hashed
        );

        for (token, term_hashed, _) in lexer {
            let term = token.as_str();

            // Check that term is not already linked to IID
            if !iid_terms_hashed.contains(&term_hashed) {
                // Prevent concurrent writes as we’re about to do a
                // read-update-write.
                // TODO: Use a merge operator.
                executor_kv_lock_write!(kv_store);

                if let Ok(term_iids) = kv_action.get_term_to_iids(term_hashed) {
                    has_commits = true;

                    // Add IID in first position in list for terms
                    let mut term_iids = term_iids.unwrap_or_default();

                    // Remove IID from list of IIDs to be popped before inserting in \
                    //   first position?
                    term_iids.retain(|&cur_iid| cur_iid != iid);

                    tracing::info!("has push executor term-to-iids: {}", iid);

                    // Make a new WriteBatch per term as it seems to yield
                    // faster `PUSH` in benchmarks.
                    // TODO: Investigate, and try to move outside for loop.
                    let mut batch = WriteBatch::default();

                    // Truncate IIDs linked to term? (ie. storage is too long)
                    let truncate_limit = self.app_conf.store.kv.retain_word_objects;

                    if term_iids.len() + 1 > truncate_limit {
                        tracing::info!(
                            "push executor term-to-iids object too long (limit: {})",
                            truncate_limit
                        );

                        // Drain overflowing IIDs (ie. oldest ones that overflow)
                        let term_iids_drain = term_iids.drain((truncate_limit - 1)..);

                        kv_action.batch_truncate_object(&mut batch, term_hashed, term_iids_drain);
                    }

                    kv_action.set_term_to_iids(
                        &mut batch,
                        term_hashed,
                        term_iids.into_iter().prepend(iid),
                    );

                    executor_ensure_op!(kv_action.write(batch));

                    // Insert term into IID to terms map
                    iid_terms_hashed.insert(term_hashed);
                } else {
                    tracing::error!("failed getting push executor term-to-iids");
                }
            }

            // Push to FST graph? (this consumes the term; to avoid sub-clones)
            if fst_action.push_word(&term, &self.app_conf.store.fst) {
                tracing::debug!("push term committed to graph: {}", term);
            }
        }

        // Commit updated list of terms for IID? (if any commit made)
        if has_commits {
            let collected_iids: Vec<StoreTermHashed> = iid_terms_hashed.into_iter().collect();

            tracing::info!(
                "has push executor iid-to-terms commits: {:?}",
                collected_iids
            );

            let mut batch = WriteBatch::default();

            kv_action.set_iid_to_terms(&mut batch, iid, collected_iids.into_iter());

            executor_ensure_op!(kv_action.write(batch));
        }

        Ok(())
    }
}
