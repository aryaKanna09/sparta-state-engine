//! SRF (State Redundancy Factor) accounting.
//!
//! SRF = logical bytes / stored bytes after chunk-level dedup and pack
//! compression. This module implements the accumulator both the scanner
//! (SRF-S, sampled) and the testkit benchmark (SRF through the engine) use, so
//! every reported number comes from the same arithmetic.

use std::collections::HashMap;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::hash::ChunkHash;
use crate::spec;

/// Aggregate dedup statistics over a set of observed objects.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DedupStats {
    /// Bytes exactly as clients wrote them (pre-normalization).
    pub logical_bytes: u64,
    /// Bytes actually chunked (post-normalization payloads).
    pub payload_bytes: u64,
    /// Sum of raw lengths of distinct chunks.
    pub unique_raw_bytes: u64,
    /// Sum of stored (compressed) lengths of distinct chunks.
    pub unique_stored_bytes: u64,
    pub object_count: u64,
    pub chunk_count: u64,
    pub unique_chunk_count: u64,
}

impl DedupStats {
    /// SRF against stored (compressed, deduped) bytes.
    pub fn srf(&self) -> f64 {
        if self.unique_stored_bytes == 0 {
            return 1.0;
        }
        self.logical_bytes as f64 / self.unique_stored_bytes as f64
    }

    /// SRF from dedup alone, before pack compression.
    pub fn srf_dedup_only(&self) -> f64 {
        if self.unique_raw_bytes == 0 {
            return 1.0;
        }
        self.logical_bytes as f64 / self.unique_raw_bytes as f64
    }

    pub fn merge(&mut self, other: &DedupStats) {
        self.logical_bytes += other.logical_bytes;
        self.payload_bytes += other.payload_bytes;
        self.unique_raw_bytes += other.unique_raw_bytes;
        self.unique_stored_bytes += other.unique_stored_bytes;
        self.object_count += other.object_count;
        self.chunk_count += other.chunk_count;
        self.unique_chunk_count += other.unique_chunk_count;
    }
}

/// Streaming accumulator with cross-object chunk dedup and optional grouped
/// breakdowns (by prefix, by content type).
pub struct DedupAccumulator {
    seen: HashSet<ChunkHash>,
    total: DedupStats,
    groups: HashMap<String, DedupStats>,
}

impl DedupAccumulator {
    pub fn new() -> Self {
        Self {
            seen: HashSet::new(),
            total: DedupStats::default(),
            groups: HashMap::new(),
        }
    }

    /// Record one object's chunks. `chunks` yields `(hash, raw_len)` in
    /// payload order; `stored_len` estimates the stored size of a chunk the
    /// first time it is seen (typically its zstd-3 compressed length).
    pub fn observe_object<F>(
        &mut self,
        logical_size: u64,
        group_keys: &[String],
        chunks: &[(ChunkHash, u64)],
        mut stored_len: F,
    ) where
        F: FnMut(&ChunkHash, u64) -> u64,
    {
        let mut delta = DedupStats {
            logical_bytes: logical_size,
            object_count: 1,
            ..Default::default()
        };
        for (hash, raw_len) in chunks {
            delta.payload_bytes += raw_len;
            delta.chunk_count += 1;
            if self.seen.insert(*hash) {
                let stored = stored_len(hash, *raw_len);
                delta.unique_raw_bytes += raw_len;
                delta.unique_stored_bytes += stored;
                delta.unique_chunk_count += 1;
            }
        }
        self.total.merge(&delta);
        for k in group_keys {
            self.groups.entry(k.clone()).or_default().merge(&delta);
        }
    }

    pub fn total(&self) -> &DedupStats {
        &self.total
    }

    pub fn groups(&self) -> &HashMap<String, DedupStats> {
        &self.groups
    }

    pub fn unique_chunks(&self) -> usize {
        self.seen.len()
    }
}

impl Default for DedupAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// Stored-size estimator used by the scanner: zstd at the spec pack level.
pub fn zstd_stored_estimate(data: &[u8]) -> u64 {
    match zstd::stream::encode_all(data, spec::PACK_ZSTD_LEVEL) {
        Ok(c) if c.len() < data.len() => c.len() as u64,
        _ => data.len() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(n: u64) -> ChunkHash {
        ChunkHash::of(&n.to_le_bytes())
    }

    #[test]
    fn duplicate_objects_multiply_srf() {
        let mut acc = DedupAccumulator::new();
        let chunks: Vec<_> = (0..10).map(|i| (h(i), 1000u64)).collect();
        for _ in 0..5 {
            acc.observe_object(10_000, &[], &chunks, |_, len| len);
        }
        let s = acc.total();
        assert_eq!(s.logical_bytes, 50_000);
        assert_eq!(s.unique_raw_bytes, 10_000);
        assert_eq!(s.unique_chunk_count, 10);
        assert!((s.srf() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn unique_objects_srf_one() {
        let mut acc = DedupAccumulator::new();
        for o in 0..4u64 {
            let chunks: Vec<_> = (0..3).map(|i| (h(o * 100 + i), 500u64)).collect();
            acc.observe_object(1500, &[], &chunks, |_, len| len);
        }
        assert!((acc.total().srf() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn group_breakdowns_track_their_objects() {
        let mut acc = DedupAccumulator::new();
        let shared: Vec<_> = (0..4).map(|i| (h(i), 100u64)).collect();
        acc.observe_object(400, &["prefix:a/".into()], &shared, |_, l| l);
        acc.observe_object(400, &["prefix:b/".into()], &shared, |_, l| l);
        let a = &acc.groups()["prefix:a/"];
        let b = &acc.groups()["prefix:b/"];
        // First group paid for the unique bytes; second got them free.
        assert_eq!(a.unique_raw_bytes, 400);
        assert_eq!(b.unique_raw_bytes, 0);
        assert_eq!(acc.total().logical_bytes, 800);
    }

    #[test]
    fn stored_estimate_never_exceeds_raw() {
        let data = vec![42u8; 100_000];
        assert!(zstd_stored_estimate(&data) < data.len() as u64);
        let mut rnd = vec![0u8; 10_000];
        let mut s = 1u64;
        for b in rnd.iter_mut() {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *b = (s >> 33) as u8;
        }
        assert!(zstd_stored_estimate(&rnd) <= rnd.len() as u64);
    }
}
