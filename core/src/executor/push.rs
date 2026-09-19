// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use rocksdb::WriteBatch;

use crate::lexer::itertools::UniqueBy;
use crate::lexer::preprocessor::{PreprocessorOutput, Token};
use crate::store::{StoreItemPart, StoreObjectOID};
use crate::util::hash::NoopU32HasherBuilder;

impl super::Executor {
    pub fn push(
        &self,
        collection: StoreItemPart,
        bucket: StoreItemPart,
        oid: StoreObjectOID,
        input: PreprocessorOutput,
        assume_new: bool,
    ) -> Result<(), ()> {
        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let (Ok(kv_store), Ok(fst_store)) = (
            self.kv_pool.acquire(true, collection, None, |_| {}),
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

        let kv_action = kv_store.access_read_write(bucket);

        let mut batch = WriteBatch::default();

        // Try to resolve existing OID to IID, otherwise initialize IID (store the \
        //   bi-directional relationship)
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
            let term_hash = token.hash();

            // Push to FST graph? (this consumes the term; to avoid sub-clones)
            if fst_store.push_word(&term, &self.app_conf.store.fst) {
                tracing::trace!("push term committed to graph: {}", term);
            }

            // Link IID to term
            kv_action.add_term_to_iids(&mut batch, term_hash, std::iter::once(iid));
        }

        // Link terms to IID
        kv_action.add_iid_to_terms(&mut batch, iid, tokens.seen().iter().copied());

        executor_ensure_op!(kv_action.write(batch));

        Ok(())
    }
}
