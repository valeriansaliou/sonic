// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::path::PathBuf;

use hf_hub::api::sync::Api;

/// Download and cache files from a Hugging Face dataset.
#[allow(dead_code)]
pub fn download_files<const N: usize>(dataset: &str, filenames: [&str; N]) -> [PathBuf; N] {
    let api = Api::new().unwrap();
    let repository = api.dataset(dataset.to_owned());

    filenames.map(|filename| repository.get(filename).unwrap())
}

/// Download and list the Parquet shards for a dataset configuration.
#[allow(dead_code)]
pub fn download_shards(dataset: &str, config: &str) -> Vec<PathBuf> {
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
