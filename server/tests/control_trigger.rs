// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

#[test]
fn trigger_consolidate() {
    let ctx = start_empty(|command| {
        command
            .env("SONIC_STORE__FST__GRAPH__CONSOLIDATE_AFTER", "3600")
            .env("SONIC_STORE__FST__POOL__INACTIVE_AFTER", "3700")
            .env("SONIC_STORE__KV__DATABASE__FLUSH_AFTER", "3600")
            .env("SONIC_STORE__KV__POOL__INACTIVE_AFTER", "3700")
    });

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let control =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    ingest
        .push("collection", "bucket", "object", "foo bar")
        .unwrap();

    let terms = search.list("collection", "bucket").unwrap();
    assert_eq!(terms.len(), 0);

    () = control.trigger_consolidate().unwrap();

    let terms = search.list("collection", "bucket").unwrap();
    assert_eq!(terms.len(), 2);
}

#[test]
fn trigger_backup() {
    let ctx = start_empty(|command| {
        command
            .env("SONIC_STORE__FST__GRAPH__CONSOLIDATE_AFTER", "3600")
            .env("SONIC_STORE__FST__POOL__INACTIVE_AFTER", "3700")
            .env("SONIC_STORE__KV__DATABASE__FLUSH_AFTER", "3600")
            .env("SONIC_STORE__KV__POOL__INACTIVE_AFTER", "3700")
    });

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let control =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    let backup_path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("test-backups")
        .join(ctx.id.to_string());
    if backup_path.exists() {
        std::fs::remove_dir_all(&backup_path).unwrap();
    }

    // NOTE: Nothing was pushed yet, so the store directories do not exist.
    () = control
        .trigger_backup(backup_path.to_str().unwrap())
        .unwrap();

    // NOTE: Pushing opens the KV store, which then stays open in the pool
    //   while the backup runs, and leaves the FST changes pending.
    ingest
        .push("collection", "bucket", "object", "foo bar")
        .unwrap();

    () = control
        .trigger_backup(backup_path.to_str().unwrap())
        .unwrap();

    let kv_backups = std::fs::read_dir(backup_path.join("kv"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(kv_backups.len(), 1);
    assert!(std::fs::read_dir(&kv_backups[0]).unwrap().next().is_some());

    std::fs::remove_dir_all(&backup_path).unwrap();
}

#[test]
#[ignore = "Not supported by sonic_client yet (FIXME)"]
fn trigger_restore() {
    todo!()
}
