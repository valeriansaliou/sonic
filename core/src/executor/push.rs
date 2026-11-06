// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashMap;

use indexmap::IndexMap;
use rocksdb::WriteBatch;

use crate::Context as _;
use crate::executor::MultipartPushContext;
use crate::lexer::preprocessor::PreprocessorOutput;
use crate::store::kv::encoding::ToKvKeyPart;
use crate::store::rocksdb::WriteOptions;
use crate::store::{Bucket, GenericOid, StoreItemPart, StoreTermHash};

#[derive(Debug, Default)]
pub struct PushOptions {
    pub is_incomplete: bool,
    pub capacity: Option<usize>,
}

impl super::Executor {
    #[tracing::instrument(
        level = "info",
        skip_all, fields(?collection, ?bucket, object = ?object_oid, fragment = ?fragment_oid, input_len = input.original_text().len()),
    )]
    pub fn push(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        object_oid: GenericOid,
        fragment_oid: Option<GenericOid>,
        input: PreprocessorOutput,
        options: PushOptions,
    ) -> Result<(), crate::Error> {
        let mut multipart_context = self.multipart_push_context.lock().unwrap();

        let (original_text, mut terms_sorted, terms): (
            &str,
            IndexMap<StoreTermHash, u8>,
            Box<dyn ExactSizeIterator<Item = &str>>,
        ) = match *multipart_context {
            // Received first chunk: initiate multipart context.
            None if options.is_incomplete => {
                let mut original_text: String =
                    String::with_capacity(options.capacity.unwrap_or(input.original_text().len()));
                original_text.push_str(input.original_text());

                let expected_term_count = if let Some(total_len) = options.capacity {
                    // NOTE: This is an approximation, supposing all chunks
                    //   will have about as many tokens as the first one.
                    (total_len / input.original_text().len()) * input.tokens().len()
                } else {
                    input.tokens().len()
                };
                let mut terms: HashMap<StoreTermHash, (Box<str>, u8)> =
                    HashMap::with_capacity(expected_term_count);

                for token in input.tokens() {
                    terms
                        .entry(token.hash())
                        .and_modify(|(_s, tf)| *tf = tf.saturating_add(1))
                        .or_insert((Box::from(token.into_normalized()), 1));
                }

                *multipart_context = Some(MultipartPushContext {
                    oid: object_oid.to_string(),
                    terms,
                    capacity: options.capacity,
                    original_text,
                });

                return Ok(());
            }

            // Normal `PUSH`.
            None => {
                let mut terms = Vec::with_capacity(input.tokens().len());

                let mut terms_sorted: IndexMap<StoreTermHash, u8> =
                    IndexMap::with_capacity(input.tokens().len());

                for token in input.tokens() {
                    match terms_sorted.entry(token.hash()) {
                        indexmap::map::Entry::Occupied(mut entry) => {
                            let tf = entry.get_mut();
                            *tf = tf.saturating_add(1);
                        }
                        indexmap::map::Entry::Vacant(entry) => {
                            terms.push(token.into_normalized());
                            entry.insert_entry(1);
                        }
                    }
                }

                // Proceed with the rest of the code.
                (
                    input.original_text(),
                    terms_sorted,
                    Box::new(terms.into_iter()),
                )
            }

            // Received multipart chunk: update multipart context.
            Some(ref mut ctx) if ctx.oid.as_str() == object_oid.as_str() => {
                if ctx.capacity.is_none() {
                    ctx.terms.reserve(input.tokens().len());
                    ctx.terms.reserve(input.tokens().len());
                }

                ctx.original_text.push_str(input.original_text());

                for token in input.tokens() {
                    ctx.terms
                        .entry(token.hash())
                        .and_modify(|(_s, tf)| *tf = tf.saturating_add(1))
                        .or_insert((Box::from(token.into_normalized()), 1));
                }

                if options.is_incomplete {
                    return Ok(());
                } else {
                    // Received last multipart chunk.
                    let terms_sorted: IndexMap<StoreTermHash, u8> =
                        ctx.terms.iter().map(|(t, (_s, tf))| (*t, *tf)).collect();

                    // Proceed with the rest of the code.
                    (
                        ctx.original_text.as_str(),
                        terms_sorted,
                        Box::new(ctx.terms.values().map(|(s, _)| s.as_ref())),
                    )
                }
            }

            // Received wrong multipart chunk.
            // NOTE: Because we store a single piece of context, we should
            //   not commit intermediate data.
            Some(ref ctx) => {
                let prev_oid = ctx.oid.clone();

                // Clear multipart context.
                *multipart_context = None;

                panic!(
                    "Last multipart chunk never received for OID {prev_oid:?}. \
                    This should not happen, something’s wrong in your code."
                );
            }
        };

        // Sometimes, a system might try to parse data that ends up containing
        // no valid token (e.g. an empty string, just punctuation or whatever).
        // To avoid writing empty values in RocksDB, let’s just abort early and
        // not lock anything at all.
        if terms_sorted.is_empty() {
            return Ok(());
        }

        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();
        let _object_read_guard = self.object_store_pool.lock_read_access();

        let kv_store = (self.kv_pool.acquire(true, collection, None))?
            .context("KV store does not exist, but it should have been created")?;
        let fst_store = self.fst_pool.acquire(collection, bucket)?;
        let object_store = (self.object_store_pool.acquire(true, collection, None))?
            .context("Object store does not exist, but it should have been created")?;

        let kv_repo = kv_store.to_repository_read_write(bucket);
        let fst_repo = fst_store.to_repository();
        let object_repo = object_store.to_repository(bucket);

        // Update KV store
        let is_new = {
            let (object_iid, object_is_new) =
                (kv_repo.get_or_assign_iid(object_oid)).context("Error getting IID")?;

            let (fragment_iid, fragment_is_new) = match fragment_oid {
                Some(fragment_oid) => {
                    (kv_repo.get_fragment_iid(object_oid, object_iid, fragment_oid))
                        .map(|(a, b)| (Some(a), Some(b)))
                        .context("Error getting IID")?
                }
                None => (None, None),
            };

            let mut batch = WriteBatch::default();

            // PERF: Sort terms by hash so they’re already sorted in the
            //   `WriteBatch`. By leveraging `memtable_insert_hint_per_batch`,
            //   we can reduce the skiplist load by ~1/3, and the total
            //   amount of computation per channel by ~14%.
            terms_sorted.sort_unstable_by_key(|term_hash, _| term_hash.to_kv_key_part());

            // Link object to term
            for (term_hash, &tf) in terms_sorted.iter() {
                kv_repo.update_term_to_iid(&mut batch, term_hash, object_iid, Some(tf));
            }

            // Link fragment to term
            if let Some(fragment_iid) = fragment_iid {
                for (term_hash, &tf) in terms_sorted.iter() {
                    kv_repo.update_term_to_fragment_iid(
                        &mut batch,
                        object_iid,
                        term_hash,
                        fragment_iid,
                        Some(tf),
                    );
                }
            }

            // Link terms to object
            if object_is_new {
                kv_repo.set_iid_to_terms(&mut batch, object_iid, terms_sorted.into_keys());
            } else {
                kv_repo.update_iid_to_terms(&mut batch, object_iid, terms_sorted.into_keys());
            }

            executor_ensure_op!(kv_repo.write_opt(batch, WriteOptions { is_ordered: true }));

            fragment_is_new.unwrap_or(object_is_new)
        };

        // Update Object store
        {
            let mut batch = WriteBatch::default();

            // Store original text
            if is_new {
                object_repo.put_object_original(
                    object_oid,
                    fragment_oid,
                    original_text,
                    &mut batch,
                );
            } else {
                object_repo.push_object_original(
                    object_oid,
                    fragment_oid,
                    original_text,
                    &mut batch,
                );
            }

            executor_ensure_op!(object_repo.write(batch));
        }

        // Update FST store
        {
            // Push to FST graph
            fst_repo.push_words(terms);
        }

        // Clear multipart context.
        *multipart_context = None;

        Ok(())
    }
}
