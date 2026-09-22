// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use rocksdb::WriteBatch;

use crate::lexer::itertools::UniqueBy;
use crate::lexer::preprocessor::{PreprocessorOutput, Token};
use crate::store::{StoreItemPart, StoreObjectOid};
use crate::util::hash::NoopU32HasherBuilder;

impl super::Executor {
    pub fn push(
        &self,
        collection: StoreItemPart,
        bucket: StoreItemPart,
        oid: StoreObjectOid,
        input: PreprocessorOutput,
        assume_new: bool,
    ) -> Result<(), ()> {
        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let kv_store = self.kv_pool.acquire(true, collection, None, |_| {})?;
        let fst_store = self.fst_pool.acquire(collection, bucket)?;

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
            let iid = (kv_action.get_new_iid(&mut batch))
                .map_err(|error| tracing::error!("Error getting new IID: {error:?}"))?;

            // Associate OID <> IID (bidirectional)
            kv_action.set_oid_to_iid(&mut batch, oid, iid);
            kv_action.set_iid_to_oid(&mut batch, iid, oid);

            Ok(iid)
        };
        let mut is_new = true;
        let iid = if assume_new {
            if let Some((last_oid, iid)) = self.last_assumed_new_oid.read().unwrap().as_ref()
                && **oid == *last_oid.as_str()
            {
                is_new = false;
                *iid
            } else {
                let iid = assign_new_iid()?;

                *self.last_assumed_new_oid.write().unwrap() = Some((oid.to_string(), iid));

                iid
            }
        } else {
            match kv_action.get_oid_to_iid(oid) {
                Ok(Some(iid)) => {
                    is_new = false;
                    iid
                }
                Ok(None) => assign_new_iid()?,
                Err(error) => {
                    tracing::error!("Error getting OID-To-IID: {error:?}");
                    assign_new_iid()?
                }
            }
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
        if assume_new && is_new {
            kv_action.set_iid_to_terms(&mut batch, iid, tokens.seen().iter().copied());
        } else {
            kv_action.add_iid_to_terms(&mut batch, iid, tokens.seen().iter().copied());
        }

        executor_ensure_op!(kv_action.write(batch));

        Ok(())
    }
}
