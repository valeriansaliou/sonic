// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::{process::Command, sync::LazyLock};

use crate::RunContext;

impl RunContext {
    pub fn print_index_size_if_enabled(&self) {
        static SHOW_INDEX_SIZE: LazyLock<bool> = LazyLock::new(|| {
            std::env::var("SHOW_INDEX_SIZE")
                .or_else(|_e| std::env::var("SHOW_STORE_SIZE"))
                .is_ok()
        });

        if *SHOW_INDEX_SIZE && let Some(ref data_path) = self.data_path {
            println!("Index size:");

            // Print size of all store files at the end of each benchmark.
            Command::new("du")
                // Show files, make it human-readable and show grand total.
                .arg("-ahc")
                .current_dir(data_path)
                .status()
                .unwrap();
        }
    }
}
