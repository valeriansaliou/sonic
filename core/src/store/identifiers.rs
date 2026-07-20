// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub type StoreBucketID = u32;
pub type StoreIIDShard = u16;
pub type StoreObjectIID = u32;
pub type StoreObjectOID<'a> = &'a str;

pub enum StoreMetaKey {
    IIDIncr,
    BucketIDIncr,
    SchemaVersion,
}

pub enum StoreMetaValue {
    IIDIncr(StoreObjectIID),
    BucketIDIncr(StoreBucketID),
    SchemaVersion(u32),
}

impl StoreMetaKey {
    pub fn as_u32(&self) -> u32 {
        match self {
            StoreMetaKey::IIDIncr => 0,
            StoreMetaKey::BucketIDIncr => 2,
            StoreMetaKey::SchemaVersion => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_converts_meta_key_to_u32() {
        assert_eq!(StoreMetaKey::IIDIncr.as_u32(), 0);
        assert_eq!(StoreMetaKey::BucketIDIncr.as_u32(), 2);
        assert_eq!(StoreMetaKey::SchemaVersion.as_u32(), 3);
    }
}
