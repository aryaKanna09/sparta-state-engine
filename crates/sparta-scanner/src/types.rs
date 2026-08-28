//! Shared scanner types.

use serde::Serialize;
use sparta_core::hash::ChunkHash;

/// One object in the enumerated population.
#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub bucket: String,
    pub key: String,
    pub size: u64,
    /// Reserved for tiering-aware reporting; populated from ListObjectsV2 and
    /// inventory but not yet surfaced.
    #[allow(dead_code)]
    pub storage_class: Option<String>,
    /// Set by prefix exclusion rules at enumeration time.
    pub excluded: bool,
}

/// One chunk observed while fingerprinting a sampled object.
#[derive(Debug, Clone)]
pub struct ChunkObs {
    pub hash: ChunkHash,
    pub raw_len: u64,
    /// zstd-3 stored-size estimate, computed the first time this scanner
    /// process saw the chunk.
    pub stored_est: u64,
}

/// The fingerprint of one sampled object.
#[derive(Debug, Clone)]
pub struct ObjectFingerprint {
    pub bucket: String,
    pub key: String,
    /// Bytes we actually fingerprinted (== object size unless truncated).
    pub logical_size: u64,
    pub full_size: u64,
    pub content_type: Option<String>,
    pub prefix_group: String,
    pub excluded: bool,
    /// Compression recognized at sniff time.
    pub detected_compression: Option<String>,
    /// True when normalization applied and `chunks` covers the raw payload.
    pub normalized: bool,
    /// Chunks of the (possibly normalized) payload.
    pub chunks: Vec<ChunkObs>,
    /// Chunks of the original bytes, kept only when `normalized` so the
    /// report can compare with/without normalization. Empty otherwise.
    pub opaque_chunks: Vec<ChunkObs>,
    pub minhash: sparta_core::minhash::MinHashSketch,
    /// True when only a prefix window of a large object was fetched.
    pub truncated: bool,
}

/// Population-level counters gathered during enumeration.
#[derive(Debug, Default, Clone, Serialize)]
pub struct PopulationStats {
    pub object_count: u64,
    pub total_bytes: u64,
    pub excluded_object_count: u64,
    pub excluded_bytes: u64,
    pub from_inventory: bool,
}
