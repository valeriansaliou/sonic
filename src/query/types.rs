// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use whatlang::Lang;

pub type QuerySearchID<'a> = &'a str;
pub type QuerySearchLimit = u16;
pub type QuerySearchOffset = u32;
pub type QuerySearchLang = Lang;

pub type QueryIngestLang = Lang;
