// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub mod huggingface;
pub mod spawn_guard;

pub mod globals {
    use std::net::Ipv6Addr;
    use std::path::{Path, PathBuf};
    use std::sync::LazyLock;

    pub const ADDR: (Ipv6Addr, u16) = (Ipv6Addr::LOCALHOST, 1491);

    // NOTE: We initialize `SONIC_BIN_PATH` lazily to avoid logging the
    //   “Environment variable "SONIC_BIN" not found” warning on
    //   `--load-baseline`.
    pub static SONIC_BIN_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
        std::env::var("SONIC_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
                    .parent()
                    .unwrap()
                    .join("release/sonic");
                eprintln!("Environment variable \"SONIC_BIN\" not found, using local build");
                path
            })
    });

    pub const SONIC_DATA_PATH: &str = concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/bench-data/",
        env!("CARGO_CRATE_NAME")
    );
}
