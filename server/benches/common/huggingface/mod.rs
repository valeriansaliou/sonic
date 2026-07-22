// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod download;
mod load;
mod wikipedia;

pub use self::download::{download_files, download_shards};
pub use self::load::iter_shard;
pub use self::wikipedia::WikipediaArticle;
