// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use rocksdb::{ColumnFamilyDescriptor, DB, IteratorMode, Options};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::keyer::{IDX_TERM_FREQUENCY, StoreKeyerBuilder, StoreKeyerHasher};
use super::kv::merge_postings;

static SECONDARY_ID: AtomicUsize = AtomicUsize::new(0);

struct SecondaryDirectory(PathBuf);

impl SecondaryDirectory {
    fn create() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "sonic-term-dump-{}-{}",
            std::process::id(),
            SECONDARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path)
            .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
        Ok(Self(path))
    }
}

impl Drop for SecondaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Dumps raw per-bucket document frequencies as term<TAB>frequency rows.
pub fn dump_term_frequencies(
    kv_root: &Path,
    collection: &str,
    output: &mut impl Write,
) -> Result<u64, String> {
    let collection_hash = StoreKeyerHasher::to_compact(collection);
    let database_path = kv_root.join(format!("{collection_hash:x}"));
    if !database_path.is_dir() {
        return Ok(0);
    }

    let options = Options::default();
    let column_families = DB::list_cf(&options, &database_path)
        .map_err(|error| format!("cannot list {}: {error}", database_path.display()))?;
    let descriptors = column_families.into_iter().map(|name| {
        let mut options = Options::default();
        if name == "postings" {
            options.set_merge_operator_associative("posting_union", merge_postings);
        }
        ColumnFamilyDescriptor::new(name, options)
    });
    let secondary = SecondaryDirectory::create()?;
    let database = DB::open_cf_descriptors_as_secondary(
        &options,
        &database_path,
        &secondary.0,
        descriptors,
    )
    .map_err(|error| format!("cannot open {}: {error}", database_path.display()))?;
    database
        .try_catch_up_with_primary()
        .map_err(|error| format!("cannot catch up {}: {error}", database_path.display()))?;
    let default_cf = database.cf_handle("default").ok_or_else(|| {
        format!(
            "default column family missing in {}",
            database_path.display()
        )
    })?;

    let mut count = 0;
    for item in database.iterator_cf(default_cf, IteratorMode::Start) {
        let (key, value) =
            item.map_err(|error| format!("cannot read {}: {error}", database_path.display()))?;
        if StoreKeyerBuilder::family(&key) != Some(IDX_TERM_FREQUENCY) {
            continue;
        }

        let route = key
            .get(5..)
            .ok_or_else(|| format!("invalid term frequency key in {}", database_path.display()))?;
        let term = StoreKeyerBuilder::decode_term_route_with_suffix(route, 0)
            .ok_or_else(|| format!("invalid term frequency key in {}", database_path.display()))?;
        let frequency = decode_frequency(&value).ok_or_else(|| {
            format!(
                "invalid frequency for {term:?} in {}",
                database_path.display()
            )
        })?;
        if term.contains(['\t', '\n', '\r']) {
            return Err(format!(
                "term {term:?} cannot be represented as TSV in {}",
                database_path.display()
            ));
        }

        writeln!(output, "{term}\t{frequency}")
            .map_err(|error| format!("cannot write output: {error}"))?;
        count += 1;
    }

    Ok(count)
}

fn decode_frequency(encoded: &[u8]) -> Option<u32> {
    encoded.try_into().ok().map(u32::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_decodes_little_endian_frequencies() {
        assert_eq!(decode_frequency(&[90, 177, 0, 0]), Some(45_402));
        assert_eq!(decode_frequency(&[1, 2, 3]), None);
    }
}
