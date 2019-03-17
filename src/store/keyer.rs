// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use std::fmt;
use std::hash::Hasher;
use std::io::Cursor;
use twox_hash::XxHash32;

use super::identifiers::*;

pub struct StoreKeyerBuilder;

pub struct StoreKeyer {
    key: StoreKeyerKey,
    page: Option<StoreKeyerPage>,
}

pub struct StoreKeyerHasher;

enum StoreKeyerIdx<'a> {
    MetaToValue(&'a StoreMetaKey),
    TermToIIDs(StoreTermHashed),
    OIDToIID(&'a StoreObjectOID),
    IIDToOID(StoreObjectIID),
    IIDToTerms(StoreObjectIID),
}

type StoreKeyerKey = [u8; KEY_CAPACITY];
type StoreKeyerPage = [u8; PAGE_CAPACITY];

const KEY_CAPACITY: usize = 5;
const PAGE_CAPACITY: usize = 2;

impl<'a> StoreKeyerIdx<'a> {
    pub fn to_index(&self) -> u8 {
        match self {
            StoreKeyerIdx::MetaToValue(_) => 0,
            StoreKeyerIdx::TermToIIDs(_) => 1,
            StoreKeyerIdx::OIDToIID(_) => 2,
            StoreKeyerIdx::IIDToOID(_) => 3,
            StoreKeyerIdx::IIDToTerms(_) => 4,
        }
    }
}

impl StoreKeyerBuilder {
    pub fn meta_to_value<'a>(meta: &'a StoreMetaKey) -> StoreKeyer {
        Self::make(StoreKeyerIdx::MetaToValue(meta), None)
    }

    pub fn term_to_iids<'a>(term_hash: StoreTermHashed, page: u16) -> StoreKeyer {
        Self::make(StoreKeyerIdx::TermToIIDs(term_hash), Some(page))
    }

    pub fn oid_to_iid<'a>(oid: &'a StoreObjectOID) -> StoreKeyer {
        Self::make(StoreKeyerIdx::OIDToIID(oid), None)
    }

    pub fn iid_to_oid<'a>(iid: StoreObjectIID) -> StoreKeyer {
        Self::make(StoreKeyerIdx::IIDToOID(iid), None)
    }

    pub fn iid_to_terms<'a>(iid: StoreObjectIID) -> StoreKeyer {
        Self::make(StoreKeyerIdx::IIDToTerms(iid), None)
    }

    fn make<'a>(idx: StoreKeyerIdx<'a>, page: Option<u16>) -> StoreKeyer {
        StoreKeyer {
            key: Self::build_key(idx),
            page: Self::build_page(page)
        }
    }

    fn build_key<'a>(idx: StoreKeyerIdx<'a>) -> StoreKeyerKey {
        // Key format: [idx<1B> | route<4B>]

        // Encode key route from u32 to array of u8 (ie. binary)
        let mut route_encoded = [0; 4];

        LittleEndian::write_u32(&mut route_encoded, Self::route_to_compact(&idx));

        // Generate final binary key
        [
            // [idx<1B>]
            idx.to_index(),
            // [route<4B>]
            route_encoded[0],
            route_encoded[1],
            route_encoded[2],
            route_encoded[3],
        ]
    }

    fn build_page<'a>(page: Option<u16>) -> Option<StoreKeyerPage> {
        // Page format: [page<2B>]

        if let Some(page) = page {
            // Encode page from u16 to array of u8 (ie. binary)
            let mut page_encoded = [0; 4];

            LittleEndian::write_u16(&mut page_encoded, page);

            // Generate final binary key
            Some([
                // [page<2B>]
                page_encoded[0],
                page_encoded[1],
            ])
        } else {
            None
        }
    }

    fn route_to_compact<'a>(idx: &StoreKeyerIdx<'a>) -> u32 {
        match idx {
            StoreKeyerIdx::MetaToValue(route) => route.as_u32(),
            StoreKeyerIdx::TermToIIDs(route) => *route,
            StoreKeyerIdx::OIDToIID(route) => StoreKeyerHasher::to_compact(route),
            StoreKeyerIdx::IIDToOID(route) => *route,
            StoreKeyerIdx::IIDToTerms(route) => *route,
        }
    }
}

impl StoreKeyer {
    pub fn to_vec(&self) -> Vec<u8> {
        // Notice: maximum capacity for [idx<1B> | route<4B>] + optional [page<2B>], which avoids \
        //   upsizing the vector capacity if we push the page (saving a memory allocation at the \
        //   cost of 2 extra bytes for keys that do not need to append the page).
        let mut bytes = Vec::with_capacity(KEY_CAPACITY + PAGE_CAPACITY);

        bytes.extend(&self.key);

        // Append page?
        if let Some(page) = self.page {
            bytes.extend(&page);
        }

        bytes
    }
}

impl StoreKeyerHasher {
    pub fn to_compact(part: &str) -> u32 {
        let mut hasher = XxHash32::with_seed(0);

        hasher.write(part.as_bytes());
        hasher.finish() as u32
    }
}

impl fmt::Display for StoreKeyer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Convert to number
        let key_route = Cursor::new(&self.key[1..5])
            .read_u32::<LittleEndian>()
            .unwrap_or(0);

        write!(f, "'{}:{:x?}' {:?}", self.key[0], key_route, self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keys_meta_to_value() {
        assert_eq!(
            &StoreKeyerBuilder::meta_to_value(&StoreMetaKey::IIDIncr).to_vec(),
            &[0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn it_keys_term_to_iids() {
        assert_eq!(
            &StoreKeyerBuilder::term_to_iids(772137347, 0).to_vec(),
            &[1, 131, 225, 5, 46, 0, 0]
        );
        assert_eq!(
            &StoreKeyerBuilder::term_to_iids(3582484684, 4).to_vec(),
            &[1, 204, 96, 136, 213, 4, 0]
        );
    }

    #[test]
    fn it_keys_oid_to_iid() {
        assert_eq!(
            &StoreKeyerBuilder::oid_to_iid(&"conversation:6501e83a".to_string()).to_vec(),
            &[2, 31, 156, 118, 213]
        );
    }

    #[test]
    fn it_keys_iid_to_oid() {
        assert_eq!(
            &StoreKeyerBuilder::iid_to_oid(10292198).to_vec(),
            &[3, 230, 11, 157, 0]
        );
    }

    #[test]
    fn it_keys_iid_to_terms() {
        assert_eq!(
            &StoreKeyerBuilder::iid_to_terms(1).to_vec(),
            &[4, 1, 0, 0, 0]
        );
        assert_eq!(
            &StoreKeyerBuilder::iid_to_terms(20).to_vec(),
            &[4, 20, 0, 0, 0]
        );
    }
}
