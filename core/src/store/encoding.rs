// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use super::*;

impl StoreObjectIid {
    #[inline]
    pub(super) const fn into_bytes(self) -> [u8; 4] {
        encode_u32(self.into_inner())
    }
}

impl StoreTermHash {
    #[inline]
    pub(super) const fn into_bytes(self) -> [u8; 4] {
        encode_u32(self.into_inner())
    }
}

#[inline]
pub(super) const fn encode_u32(decoded: u32) -> [u8; 4] {
    decoded.to_le_bytes()
}

#[inline]
pub(super) fn decode_u32_mapped<T: From<u32>>(encoded: &[u8]) -> Result<T, ()> {
    decode_u32(encoded).map(T::from)
}

pub(super) const fn decode_u32(encoded: &[u8]) -> Result<u32, ()> {
    if encoded.len() == 4 {
        Ok(u32::from_le_bytes([
            encoded[0], encoded[1], encoded[2], encoded[3],
        ]))
    } else {
        Err(())
    }
}

pub(super) fn encode_u32_list_mapped<T: Into<u32>>(
    decoded: impl ExactSizeIterator<Item = T>,
) -> Vec<u8> {
    // Pre-reserve required capacity as to avoid heap resizes (50%
    // performance gain relative to initializing this with a zero-capacity)
    let mut encoded = Vec::with_capacity(decoded.len() * 4);

    for decoded_item in decoded {
        encoded.extend(&encode_u32(decoded_item.into()))
    }

    encoded
}

pub(super) fn decode_u32_list_mapped<T: From<u32>>(encoded: &[u8]) -> Result<Vec<T>, ()> {
    // Pre-reserve required capacity as to avoid heap resizes (50%
    // performance gain relative to initializing this with a zero-capacity)
    let mut decoded = Vec::with_capacity(encoded.len() / 4);

    for encoded_chunk in encoded.chunks(4) {
        match decode_u32_mapped(encoded_chunk) {
            Ok(decoded_chunk) => decoded.push(decoded_chunk),
            Err(()) => return Err(()),
        }
    }

    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_encodes_atom() {
        assert_eq!(encode_u32(0), [0, 0, 0, 0]);
        assert_eq!(encode_u32(1), [1, 0, 0, 0]);
        assert_eq!(encode_u32(45402), [90, 177, 0, 0]);
    }

    #[test]
    fn it_decodes_atom() {
        assert_eq!(decode_u32(&[0, 0, 0, 0]), Ok(0));
        assert_eq!(decode_u32(&[1, 0, 0, 0]), Ok(1));
        assert_eq!(decode_u32(&[90, 177, 0, 0]), Ok(45402));
    }

    #[test]
    fn it_encodes_atom_list() {
        assert_eq!(
            encode_u32_list([0, 2, 3].into_iter()),
            [0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]
        );
        assert_eq!(encode_u32_list([45402].into_iter()), [90, 177, 0, 0]);
    }

    #[test]
    fn it_decodes_atom_list() {
        assert_eq!(
            decode_u32_list(&[0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]),
            Ok(vec![0, 2, 3])
        );
        assert_eq!(decode_u32_list(&[90, 177, 0, 0]), Ok(vec![45402]));
    }

    // MARK: Helpers

    #[inline(always)]
    fn encode_u32_list(decoded: impl ExactSizeIterator<Item = u32>) -> Vec<u8> {
        encode_u32_list_mapped(decoded)
    }

    #[inline(always)]
    fn decode_u32_list(encoded: &[u8]) -> Result<Vec<u32>, ()> {
        decode_u32_list_mapped(encoded)
    }
}

#[cfg(all(feature = "benchmark", test))]
mod benches {
    extern crate test;

    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_encode_atom(b: &mut Bencher) {
        b.iter(|| encode_u32(0));
    }

    #[bench]
    fn bench_decode_atom(b: &mut Bencher) {
        let encoded_atom = [0, 0, 0, 0];

        b.iter(|| decode_u32(&encoded_atom));
    }

    #[bench]
    fn bench_encode_atom_list(b: &mut Bencher) {
        let atom_list = [0, 2, 3];

        b.iter(|| encode_u32_list(&atom_list));
    }

    #[bench]
    fn bench_decode_atom_list(b: &mut Bencher) {
        let encoded_atom_list = [0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0];

        b.iter(|| decode_u32_list(&encoded_atom_list));
    }
}
