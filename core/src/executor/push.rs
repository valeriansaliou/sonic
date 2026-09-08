// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashSet;
use rocksdb::WriteBatch;
use std::sync::Arc;

use crate::lexer::TokenLexer;
use crate::store::StoreItem;
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder};
use crate::util::hash::NoopU32HasherBuilder;

impl super::Executor {
    pub fn push(&self, item: StoreItem, lexer: TokenLexer, assume_new: bool) -> Result<(), ()> {
        let StoreItem(collection, Some(bucket), Some(object)) = item else {
            return Err(());
        };

        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let (Ok(kv_store), Ok(fst_store)) = (
            self.kv_pool
                .acquire(StoreKVAcquireMode::Any, collection, None, |_| {}),
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

        let mut batch = WriteBatch::default();

        // Try to resolve existing OID to IID, otherwise initialize IID (store the \
        //   bi-directional relationship)
        let oid = object.as_str();
        let write_guard = kv_store.lock.write().unwrap();
        let assign_new_iid = || {
            tracing::trace!("must initialize push executor oid-to-iid and iid-to-oid");

            // Bump last stored increment
            match kv_action.auto_increment_iid(Some(write_guard)) {
                Ok(iid) => {
                    // Associate OID <> IID (bidirectional)
                    kv_action.set_oid_to_iid(&mut batch, oid, iid);
                    kv_action.set_iid_to_oid(&mut batch, iid, oid);

                    Some(iid)
                }
                Err(error) => {
                    tracing::error!("{error}");

                    None
                }
            }
        };
        let iid = if assume_new {
            assign_new_iid()
        } else {
            (kv_action.get_oid_to_iid(oid))
                .unwrap_or_else(|()| {
                    tracing::error!("Error getting OID-To-IID");
                    None
                })
                .or_else(assign_new_iid)
        };

        let Some(iid) = iid else {
            return Err(());
        };

        let mut tokens = HashSet::with_capacity_and_hasher(128, NoopU32HasherBuilder);

        for (token, term_hashed, _) in lexer {
            let term = token.as_str();

            tokens.insert(term_hashed);

            // Push to FST graph? (this consumes the term; to avoid sub-clones)
            if fst_action.push_word(&term, &self.app_conf.store.fst) {
                tracing::trace!("push term committed to graph: {}", term);
            }
        }

        for &term_hashed in tokens.iter() {
            // Link IID to term
            kv_action.add_term_to_iids(&mut batch, term_hashed, std::iter::once(iid));
        }

        // Link terms to IID
        kv_action.add_iid_to_terms(&mut batch, iid, tokens.into_iter());

        executor_ensure_op!(kv_action.write(batch));

        Ok(())
    }
}
