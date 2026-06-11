// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::path::{Path, PathBuf};
use std::process::Command;

/// Download and list Huggingface dataset shards, skipping already downloaded
/// data.
pub fn download_shards(dataset: &str, config: &str) -> Vec<PathBuf> {
    let dataset_dir = dataset_dir(dataset);
    if !dataset_dir.exists() {
        std::fs::create_dir_all(&dataset_dir).unwrap();
    }

    let urls = list_shards(dataset, config);

    let mut shards = Vec::with_capacity(urls.len());

    for url in urls {
        let filename = url.rsplit_once('/').unwrap().1;
        let out = dataset_dir.join(filename);

        if out.exists() {
            tracing::debug!(
                ?dataset,
                "Shard already exists: {:?}",
                out.file_name().unwrap()
            );
        } else {
            let status = Command::new("wget").arg(url).arg("-O").arg(&out).status();
            assert!(status.unwrap().success());
        }

        shards.push(out);
    }

    shards
}

// MARK: Helpers

fn download_dataset(dataset: &str) -> PathBuf {
    let out = parquet_file_path(dataset);

    if out.exists() {
        tracing::debug!(
            "Dataset metadata already exists: {:?}",
            out.file_name().unwrap()
        );
        return out;
    }

    let status = Command::new("wget")
        .arg(format!(
            "https://datasets-server.huggingface.co/parquet?dataset={dataset}"
        ))
        .arg("-O")
        .arg(&out)
        .status();
    assert!(status.unwrap().success());

    out
}

fn list_shards(dataset: &str, config: &str) -> Vec<String> {
    let parquet_file = download_dataset(dataset);

    let output = Command::new("jq")
        .arg("-r")
        .arg(format!(
            ".parquet_files[] | select(.config == {config:?}) | .url"
        ))
        .arg(&parquet_file)
        .stderr(std::process::Stdio::inherit())
        .output()
        .unwrap();
    assert!(output.status.success());

    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

fn dataset_dir(dataset: &str) -> PathBuf {
    // Don’t try path traversal :)
    assert!(!dataset.contains("."));

    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("huggingface")
        .join(dataset)
}

fn parquet_file_path(dataset: &str) -> PathBuf {
    dataset_dir(dataset).with_added_extension("parquet")
}
