//! MinHash similarity sketches over chunk hash sets.
//!
//! Used for parent detection: two object versions that share most chunks have
//! a high Jaccard similarity, which MinHash estimates from fixed-size
//! signatures without comparing chunk lists. 128 permutations (spec constant),
//! each a splitmix64-keyed mix of the chunk hash's 64-bit prefix. Banded into
//! 32 bands of 4 rows for LSH candidate lookup in the scanner.

use serde::{Deserialize, Serialize};

use crate::hash::ChunkHash;
use crate::spec;

/// Rows per LSH band (32 bands x 4 rows = 128 permutations).
pub const BAND_ROWS: usize = 4;

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

/// Permutation seed for row `i`; fixed so sketches are comparable across
/// processes and across implementations of the SRF spec.
fn seed(i: usize) -> u64 {
    splitmix64(0x5041525441_u64 ^ (i as u64).wrapping_mul(0xa076_1d64_78bd_642f))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinHashSketch {
    sig: Vec<u64>,
}

impl MinHashSketch {
    pub fn new() -> Self {
        Self {
            sig: vec![u64::MAX; spec::MINHASH_PERMS],
        }
    }

    pub fn insert(&mut self, hash: &ChunkHash) {
        let x = hash.prefix_u64();
        for (i, slot) in self.sig.iter_mut().enumerate() {
            let v = splitmix64(x ^ seed(i));
            if v < *slot {
                *slot = v;
            }
        }
    }

    pub fn from_hashes<'a>(hashes: impl IntoIterator<Item = &'a ChunkHash>) -> Self {
        let mut s = Self::new();
        for h in hashes {
            s.insert(h);
        }
        s
    }

    pub fn signature(&self) -> &[u64] {
        &self.sig
    }

    pub fn from_signature(sig: Vec<u64>) -> Option<Self> {
        (sig.len() == spec::MINHASH_PERMS).then_some(Self { sig })
    }

    /// Estimated Jaccard similarity of the underlying chunk sets.
    pub fn jaccard(&self, other: &Self) -> f64 {
        let matching = self
            .sig
            .iter()
            .zip(&other.sig)
            .filter(|(a, b)| a == b)
            .count();
        matching as f64 / self.sig.len() as f64
    }

    /// LSH band keys: objects sharing any band key are similarity candidates.
    pub fn band_keys(&self) -> Vec<u64> {
        self.sig
            .chunks(BAND_ROWS)
            .enumerate()
            .map(|(band, rows)| {
                let mut acc = splitmix64(band as u64 ^ 0x42414e44);
                for r in rows {
                    acc = splitmix64(acc ^ *r);
                }
                acc
            })
            .collect()
    }
}

impl Default for MinHashSketch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashes(seed: u64, n: usize) -> Vec<ChunkHash> {
        (0..n)
            .map(|i| {
                let mut buf = [0u8; 16];
                buf[..8].copy_from_slice(&seed.to_le_bytes());
                buf[8..].copy_from_slice(&(i as u64).to_le_bytes());
                ChunkHash::of(&buf)
            })
            .collect()
    }

    #[test]
    fn identical_sets_full_similarity() {
        let hs = hashes(1, 200);
        let a = MinHashSketch::from_hashes(&hs);
        let b = MinHashSketch::from_hashes(&hs);
        assert_eq!(a.jaccard(&b), 1.0);
    }

    #[test]
    fn disjoint_sets_low_similarity() {
        let a = MinHashSketch::from_hashes(&hashes(1, 200));
        let b = MinHashSketch::from_hashes(&hashes(2, 200));
        assert!(a.jaccard(&b) < 0.1, "got {}", a.jaccard(&b));
    }

    #[test]
    fn overlap_estimate_tracks_true_jaccard() {
        // 300 shared + 100 unique each side => true J = 300/500 = 0.6.
        let shared = hashes(7, 300);
        let mut xs = shared.clone();
        xs.extend(hashes(8, 100));
        let mut ys = shared;
        ys.extend(hashes(9, 100));
        let est = MinHashSketch::from_hashes(&xs).jaccard(&MinHashSketch::from_hashes(&ys));
        assert!((est - 0.6).abs() < 0.15, "estimate {est} too far from 0.6");
    }

    #[test]
    fn similar_objects_share_band_keys() {
        // 98% overlap: expect many shared bands.
        let shared = hashes(3, 490);
        let mut xs = shared.clone();
        xs.extend(hashes(4, 10));
        let mut ys = shared;
        ys.extend(hashes(5, 10));
        let a = MinHashSketch::from_hashes(&xs).band_keys();
        let b = MinHashSketch::from_hashes(&ys).band_keys();
        let common = a.iter().zip(&b).filter(|(x, y)| x == y).count();
        assert!(common >= 10, "only {common} of {} bands match", a.len());
    }

    #[test]
    fn sketch_is_order_independent() {
        let mut hs = hashes(11, 100);
        let a = MinHashSketch::from_hashes(&hs);
        hs.reverse();
        let b = MinHashSketch::from_hashes(&hs);
        assert_eq!(a, b);
    }
}
