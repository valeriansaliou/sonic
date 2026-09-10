// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// Copyright: 2026, Baptiste Jamin <baptiste@crisp.chat>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(dead_code)]

use std::path::PathBuf;

use hf_hub::api::sync::Api;

/// Download and cache files from a Hugging Face dataset.
pub fn download_files<const N: usize>(dataset: &str, filenames: [&str; N]) -> [PathBuf; N] {
    let api = Api::new().unwrap();
    let repository = api.dataset(dataset.to_owned());

    filenames.map(|filename| repository.get(filename).unwrap())
}

/// Download and list the Parquet shards for a dataset configuration.
pub fn download_shards(dataset: &str, config: &str, limit: Option<usize>) -> Vec<PathBuf> {
    let cache = hf_hub::Cache::from_env();
    if let Some(cache_path) = cache.dataset(dataset.to_owned()).get(config) {
        let shards: Vec<PathBuf> = std::fs::read_dir(cache_path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .take(limit.unwrap_or(usize::MAX))
            .collect();

        // NOTE: There would be a bug if only n shards have been downloaded
        //   then we ask for unlimited. But we don’t care about this edge case,
        //   offline support is better. To fix it we could check the name of
        //   the parquet file (e.g. `train-00001-of-00041.parquet`).
        if limit.is_none_or(|limit| shards.len() == limit) {
            return shards;
        }
    }

    let api = Api::new().unwrap();
    let repository = api.dataset(dataset.to_owned());
    let prefix = format!("{config}/");
    let mut filenames: Vec<_> = repository
        .info()
        .unwrap()
        .siblings
        .into_iter()
        .map(|file| file.rfilename)
        .filter(|filename| filename.starts_with(&prefix) && filename.ends_with(".parquet"))
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    filenames.sort_unstable();

    assert!(
        !filenames.is_empty(),
        "No Parquet shards found for {dataset:?} configuration {config:?}"
    );
    filenames
        .iter()
        .map(|filename| repository.get(filename).unwrap())
        .collect()
}
