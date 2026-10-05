// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, DualFroz <me@dualfroz.com>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Data backup

mod common;

use crate::common::util::unique_hex;
use crate::common::*;

/// Consolidated FST stores are backed up, and can be restored.
#[test]
fn test_fst_backup_and_restore() {
    init_logging();
    let executor = make_test_executor(|_| {});

    exec!(executor -> PUSH "messages" "user:1" "chat:1" "Hello world");
    exec!(executor -> TRIGGER consolidate);

    let backup_path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(unique_hex().unwrap())
        .join("fst-backup");

    executor.fst_pool.backup(&backup_path).unwrap();

    let backup_files = std::fs::read_dir(&backup_path)
        .unwrap()
        .flat_map(|collection| std::fs::read_dir(collection.unwrap().path()).unwrap())
        .map(|bucket| bucket.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(backup_files.len(), 1, "got: {backup_files:?}");
    assert!(
        backup_files[0].to_string_lossy().ends_with(".fst.bck"),
        "got: {backup_files:?}"
    );

    let restored_executor = make_test_executor(|_| {});

    restored_executor.fst_pool.restore(&backup_path).unwrap();

    let response = exec!(restored_executor -> LIST "messages" "user:1");
    assert_contains!(response, ["hello", "world"]);
}
