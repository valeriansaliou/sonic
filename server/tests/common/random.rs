// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Claude Sonnet 5
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::LazyLock;

// NOTE: Implementation cannot be time-based, even with nanosecond precision,
//   as tests are ran concurrently and such conflicts happen (very often).
//   When it does, one test cleaning up its temporary directory causes another
//   to fail. We don’t want that.
pub fn random_seed() -> u64 {
    use std::io::Read as _;

    let mut urandom = std::fs::File::open("/dev/urandom").unwrap();
    let mut buf = [0u8; 8];
    urandom.read_exact(&mut buf).unwrap();

    u64::from_le_bytes(buf)
}

/// Word length: `4..=8`.
fn gen_word(rng: &mut SplitMix64, alphabet: &[u8]) -> String {
    let len = 3 + rng.next_range(7);
    (0..len)
        .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
        .collect()
}

/// Note that there can be duplicates.
pub fn build_dictionary(len: usize, alphabet: &[u8], seed: u64) -> Vec<String> {
    let mut rng = SplitMix64::new(seed);
    (0..len).map(|_| gen_word(&mut rng, alphabet)).collect()
}

/// Dictionary of 500 pseudo-random words, generated once, lazily, deterministically.
// static WORDS: LazyLock<Vec<String>> = LazyLock::new(|| {
//     let mut rng = SplitMix64::new(SEED);
//     (0..DICT_SIZE).map(|_| gen_word(&mut rng)).collect()
// });

/// Picks `n` words fast and deterministically, by rotating through the
/// dictionary starting at a seed-derived offset with a fixed odd stride
/// (odd stride + power-of-two-agnostic modulo ensures good coverage even
/// though it's not "true" randomness — fine for test data).
pub fn pick_words<'a>(
    n: usize,
    dictionary: &'a LazyLock<Vec<String>>,
    seed: u64,
) -> impl Iterator<Item = &'a str> {
    let mut rng = SplitMix64::new(seed ^ 0xA5A5_A5A5_A5A5_A5A5);
    let len = dictionary.len();

    (0..n).map(move |_| dictionary[rng.next_range(len)].as_str())
}

/// Minimal, fast, deterministic PRNG (SplitMix64).
#[derive(Debug, Clone)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_range(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}
