// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use rocksdb::WriteBatch;

use crate::lexer::itertools::UniqueBy;
use crate::lexer::preprocessor::{PreprocessorOutput, Token};
use crate::store::StoreItem;
use crate::store::kv::{StoreKVAcquireMode, StoreKVPool};
use crate::util::hash::NoopU32HasherBuilder;

impl super::Executor {
    pub fn push(
        &self,
        item: StoreItem,
        input: PreprocessorOutput,
        assume_new: bool,
    ) -> Result<(), ()> {
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

        let kv_action = StoreKVPool::access_read_write(bucket, &kv_store);

        let mut batch = WriteBatch::default();

        // Try to resolve existing OID to IID, otherwise initialize IID (store the \
        //   bi-directional relationship)
        let oid = object.as_str();
        let mut assign_new_iid = || {
            tracing::trace!("must initialize push executor oid-to-iid and iid-to-oid");

            // Bump last stored increment
            let iid = kv_action.get_new_iid(&mut batch);

            // Associate OID <> IID (bidirectional)
            kv_action.set_oid_to_iid(&mut batch, oid, iid);
            kv_action.set_iid_to_oid(&mut batch, iid, oid);

            iid
        };
        let iid = if assume_new {
            assign_new_iid()
        } else {
            (kv_action.get_oid_to_iid(oid))
                .unwrap_or_else(|()| {
                    tracing::error!("Error getting OID-To-IID");
                    None
                })
                .unwrap_or_else(assign_new_iid)
        };

        let mut tokens =
            UniqueBy::new_with_hasher(input.tokens(), Token::hash, NoopU32HasherBuilder);

        for token in &mut tokens {
            let term = token.as_normalized();
            let term_hashed = token.hash();

            // Push to FST graph? (this consumes the term; to avoid sub-clones)
            if fst_store.push_word(&term, &self.app_conf.store.fst) {
                tracing::trace!("push term committed to graph: {}", term);
            }

            // Link IID to term
            kv_action.add_term_to_iids(&mut batch, term_hashed, std::iter::once(iid));
        }

        // Link terms to IID
        kv_action.add_iid_to_terms(&mut batch, iid, tokens.seen().iter().copied());

        executor_ensure_op!(kv_action.write(batch));

        Ok(())
    }
}
