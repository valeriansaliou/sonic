// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashMap;

use rocksdb::WriteBatch;

use crate::executor::MultipartPushContext;
use crate::lexer::itertools::UniqueBy;
use crate::lexer::preprocessor::{PreprocessorOutput, Token};
use crate::store::kv::KvRepositoryReadWrite;
use crate::store::{Bucket, StoreItemPart, StoreObjectIid, StoreObjectOid, StoreTermHash};
use crate::util::hash::NoopU32HasherBuilder;

#[derive(Debug, Default)]
pub struct PushOptions {
    pub assume_new: bool,
    pub is_incomplete: bool,
    pub capacity: Option<usize>,
}

impl super::Executor {
    pub fn push(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        oid: StoreObjectOid,
        input: PreprocessorOutput,
        options: PushOptions,
    ) -> Result<(), ()> {
        let mut tokens =
            UniqueBy::new_with_hasher(input.tokens(), Token::hash, NoopU32HasherBuilder);

        let mut multipart_context = self.multipart_push_context.lock().unwrap();

        if options.is_incomplete {
            return match *multipart_context {
                // Received first chunk: initiate multipart context.
                None => {
                    let expected_term_count = if let Some(total_len) = options.capacity {
                        // NOTE: This is an approximation, supposing all chunks
                        //   will have about as many tokens as the first one.
                        (total_len / input.original_text().len()) * input.tokens().len()
                    } else {
                        input.tokens().len()
                    };
                    let mut terms: HashMap<StoreTermHash, Box<str>> =
                        HashMap::with_capacity(expected_term_count);

                    for token in tokens {
                        terms.insert(token.hash(), Box::from(token.into_normalized()));
                    }

                    *multipart_context = Some(MultipartPushContext {
                        oid: oid.to_string(),
                        // `NEW` in any multipart chunk is considered `NEW` on commit.
                        assume_new: options.assume_new,
                        terms,
                        capacity: options.capacity,
                    });

                    Ok(())
                }

                // Received intermediate chunk: update multipart context.
                Some(ref mut ctx) => {
                    // `NEW` in any multipart chunk is considered `NEW` on commit.
                    ctx.assume_new |= options.assume_new;

                    if ctx.capacity.is_none() {
                        ctx.terms.reserve(input.tokens().len());
                    }

                    for token in tokens {
                        ctx.terms
                            .insert(token.hash(), Box::from(token.into_normalized()));
                    }

                    Ok(())
                }
            };
        }

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

        let kv_repo = kv_store.to_repository_read_write(bucket);
        let fst_repo = fst_store.to_repository();

        fn assign_new_iid(
            oid: StoreObjectOid<'_>,
            kv_repo: &KvRepositoryReadWrite<'_>,
            batch: &mut WriteBatch,
        ) -> Result<StoreObjectIid, ()> {
            tracing::trace!("Must initialize push executor oid-to-iid and iid-to-oid");

            // Bump last stored increment
            let iid = (kv_repo.get_new_iid(batch))
                .map_err(|error| tracing::error!("Error getting new IID: {error:?}"))?;

            // Associate OID <> IID (bidirectional)
            kv_repo.set_oid_to_iid(batch, oid, iid);
            kv_repo.set_iid_to_oid(batch, iid, oid);

            Ok(iid)
        }

        fn get_iid(
            oid: StoreObjectOid<'_>,
            assume_new: bool,
            kv_repo: &KvRepositoryReadWrite<'_>,
            batch: &mut WriteBatch,
        ) -> Result<(StoreObjectIid, bool), ()> {
            if assume_new {
                // Get new IID (assume new).
                assign_new_iid(oid, kv_repo, batch).map(|iid| (iid, true))
            } else {
                // Try to resolve existing OID to IID, otherwise get new IID.
                match kv_repo.get_oid_to_iid(oid) {
                    Ok(Some(iid)) => Ok((iid, false)),
                    Ok(None) => assign_new_iid(oid, kv_repo, batch).map(|iid| (iid, true)),
                    Err(error) => {
                        tracing::error!("Error getting OID-To-IID: {error:?}");
                        assign_new_iid(oid, kv_repo, batch).map(|iid| (iid, true))
                    }
                }
            }
        }

        match *multipart_context {
            // Received last multipart chunk.
            Some(ref mut ctx) if ctx.oid.as_str() == oid.as_str() => {
                // `NEW` in any multipart chunk is considered `NEW` on commit.
                ctx.assume_new |= options.assume_new;

                if ctx.capacity.is_none() {
                    ctx.terms.reserve(input.tokens().len());
                }

                for token in tokens {
                    ctx.terms
                        .insert(token.hash(), Box::from(token.into_normalized()));
                }

                // Update KV store
                {
                    let mut batch = WriteBatch::default();

                    let (iid, is_new) = get_iid(oid, ctx.assume_new, &kv_repo, &mut batch)?;

                    for term_hash in ctx.terms.keys() {
                        // Link IID to term
                        kv_repo.add_term_to_iid(&mut batch, *term_hash, iid);
                    }

                    // Link terms to IID
                    if is_new {
                        kv_repo.set_iid_to_terms(&mut batch, iid, ctx.terms.keys().copied());
                    } else {
                        kv_repo.add_iid_to_terms(&mut batch, iid, ctx.terms.keys().copied());
                    }

                    executor_ensure_op!(kv_repo.write(batch));
                }

                // Update FST store
                {
                    // Push to FST graph
                    fst_repo.push_words(
                        ctx.terms.values().map(Box::as_ref),
                        &self.app_conf.store.fst,
                    );
                }

                // Clear multipart context.
                *multipart_context = None;

                Ok(())
            }

            // Received wrong last multipart chunk.
            // NOTE: Because we store a single piece of context, we should
            //   not commit intermediate data “”
            Some(ref ctx) => {
                let prev_oid = ctx.oid.clone();

                // Clear multipart context.
                *multipart_context = None;

                panic!(
                    "Last multipart chunk never received for OID {prev_oid:?}. \
                        This should not happen, something’s wrong in your code."
                );
            }

            // Normal `PUSH`.
            None => {
                let mut terms = Vec::with_capacity(input.tokens().len());

                // Update KV store
                {
                    let mut batch = WriteBatch::default();

                    let (iid, is_new) = get_iid(oid, options.assume_new, &kv_repo, &mut batch)?;

                    for token in &mut tokens {
                        let term_hash = token.hash();

                        // Link IID to term
                        kv_repo.add_term_to_iid(&mut batch, term_hash, iid);

                        terms.push(token.into_normalized());
                    }

                    // Link terms to IID
                    if is_new {
                        kv_repo.set_iid_to_terms(&mut batch, iid, tokens.seen().iter().copied());
                    } else {
                        kv_repo.add_iid_to_terms(&mut batch, iid, tokens.seen().iter().copied());
                    }

                    executor_ensure_op!(kv_repo.write(batch));
                }

                // Update FST store
                {
                    // Push to FST graph
                    fst_repo.push_words(terms.into_iter(), &self.app_conf.store.fst);
                }

                Ok(())
            }
        }
    }
}
