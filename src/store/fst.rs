// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub struct StoreFSTBuilder;
pub struct StoreFST;

impl StoreFSTBuilder {
    pub fn new() -> Result<StoreFST, &'static str> {
        Ok(StoreFST {})
    }
}
