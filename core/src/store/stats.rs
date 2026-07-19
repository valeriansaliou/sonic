// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StoreCollectionStats {
    pub collection: String,
    pub schema_version: u32,
    pub index: StoreColumnFamilyStats,
    pub postings: StoreColumnFamilyStats,
    pub documents: StoreColumnFamilyStats,
    pub logical: Option<StoreLogicalStats>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StoreColumnFamilyStats {
    pub live_data_bytes: u64,
    pub sst_bytes: u64,
    pub memtable_bytes: u64,
    pub estimated_keys: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StoreLogicalStats {
    pub index_key_bytes: u64,
    pub index_value_bytes: u64,
    pub document_key_bytes: u64,
    pub document_encoded_bytes: u64,
    pub document_text_bytes: u64,
    pub document_metadata_bytes: u64,
    pub document_count: u64,
    pub term_postings: StorePostingStats,
    pub time_postings: StorePostingStats,
    pub families: Vec<StoreIndexFamilyStats>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StorePostingStats {
    pub fragments: u64,
    pub sparse_fragments: u64,
    pub dense_fragments: u64,
    pub encoded_bytes: u64,
    pub associations: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StoreIndexFamilyStats {
    pub index: u8,
    pub name: String,
    pub keys: u64,
    pub key_bytes: u64,
    pub value_bytes: u64,
}
