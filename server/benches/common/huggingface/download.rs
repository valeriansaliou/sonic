// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::path::PathBuf;

use hf_hub::repository::RepoTreeEntry;
use hf_hub::{HFClientSync, split_id};

/// Download and cache files from a Hugging Face dataset.
#[allow(dead_code)]
pub fn download_files<const N: usize>(dataset: &str, filenames: [&str; N]) -> [PathBuf; N] {
    let client = HFClientSync::new().unwrap();
    let (owner, name) = split_id(dataset);
    let repository = client.dataset(owner, name);

    filenames.map(|filename| {
        repository
            .download_file()
            .filename(filename)
            .send()
            .unwrap()
    })
}

/// Download and list the Parquet shards for a dataset configuration.
#[allow(dead_code)]
pub fn download_shards(dataset: &str, config: &str) -> Vec<PathBuf> {
    let client = HFClientSync::new().unwrap();
    let (owner, name) = split_id(dataset);
    let repository = client.dataset(owner, name);
    let prefix = format!("{config}/");
    let mut filenames: Vec<_> = repository
        .list_tree()
        .recursive(true)
        .send()
        .unwrap()
        .into_iter()
        .filter_map(|entry| match entry {
            RepoTreeEntry::File { path, .. } => Some(path),
            RepoTreeEntry::Directory { .. } => None,
        })
        .filter(|filename| filename.starts_with(&prefix) && filename.ends_with(".parquet"))
        .collect();
    filenames.sort_unstable();

    assert!(
        !filenames.is_empty(),
        "No Parquet shards found for {dataset:?} configuration {config:?}"
    );
    filenames
        .iter()
        .map(|filename| {
            repository
                .download_file()
                .filename(filename)
                .send()
                .unwrap()
        })
        .collect()
}
