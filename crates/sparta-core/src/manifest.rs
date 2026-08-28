//! Manifest format v1.
//!
//! A manifest describes one object version written through the engine: the
//! ordered chunk list covering its (possibly normalization-expanded) payload,
//! original metadata, the recompression recipe if normalization applied, and
//! optional lineage. Manifests are JSON objects stored under `_sparta/` and,
//! together with pack tail indexes, are sufficient to reconstruct every object
//! with no local database at all.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::compression::RecompressionRecipe;
use crate::hash::ChunkHash;
use crate::{spec, CoreError};

/// A reference to one stored chunk, including its pack location. Pack
/// locations are stable for the life of the manifest: GC only deletes packs
/// once no manifest references any chunk in them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRef {
    pub hash: ChunkHash,
    pub raw_len: u64,
    pub pack_id: String,
    pub offset: u64,
    pub data_len: u64,
    pub codec: u8,
    #[serde(default)]
    pub dict_id: u64,
}

/// One object version as stored by the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    pub bucket: String,
    pub key: String,
    #[serde(default)]
    pub version_id: Option<String>,
    /// Size of the object exactly as the client PUT it.
    pub logical_size: u64,
    /// BLAKE3 of the object exactly as the client PUT it. Every GET verifies
    /// the reconstructed stream against this before the final byte is served.
    pub object_hash: ChunkHash,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub user_metadata: BTreeMap<String, String>,
    /// ETag reported to clients (upstream-compatible where possible).
    #[serde(default)]
    pub etag: Option<String>,
    /// Compression recognized at ingest, normalized or not.
    #[serde(default)]
    pub detected_compression: Option<String>,
    /// Present iff normalization applied; chunks then cover the *raw* payload.
    #[serde(default)]
    pub recipe: Option<RecompressionRecipe>,
    /// Size of the chunked payload (== logical_size when no recipe).
    pub payload_size: u64,
    /// BLAKE3 of the chunked payload (== object_hash when no recipe).
    pub payload_hash: ChunkHash,
    pub chunks: Vec<ChunkRef>,
    /// Lineage: manifest locator of the declared/detected parent (M5).
    #[serde(default)]
    pub parent: Option<String>,
    /// MinHash signature over chunk hashes, for parent detection.
    #[serde(default)]
    pub minhash: Option<Vec<u64>>,
    /// Wall-clock creation time, supplied by the caller (core keeps no clock).
    pub created_unix_ms: u64,
    /// Set when a GET-time verification failed; quarantined manifests are
    /// never served again and are surfaced in operations tooling.
    #[serde(default)]
    pub quarantined: bool,
}

impl Manifest {
    pub fn to_json(&self) -> Result<Vec<u8>, CoreError> {
        Ok(serde_json::to_vec_pretty(self)?)
    }

    pub fn from_json(data: &[u8]) -> Result<Self, CoreError> {
        let m: Manifest = serde_json::from_slice(data)?;
        m.validate()?;
        Ok(m)
    }

    /// Structural validation: chunk lengths must tile the payload exactly.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.format_version != spec::FORMAT_VERSION {
            return Err(CoreError::Manifest(format!(
                "unsupported manifest version {}",
                self.format_version
            )));
        }
        let sum: u64 = self.chunks.iter().map(|c| c.raw_len).sum();
        if sum != self.payload_size {
            return Err(CoreError::Manifest(format!(
                "chunk lengths sum to {sum} but payload_size is {}",
                self.payload_size
            )));
        }
        if self.recipe.is_none()
            && (self.payload_size != self.logical_size || self.payload_hash != self.object_hash)
        {
            return Err(CoreError::Manifest(
                "no recipe but payload differs from logical object".into(),
            ));
        }
        for c in &self.chunks {
            if c.raw_len == 0 {
                return Err(CoreError::Manifest("zero-length chunk reference".into()));
            }
            if c.pack_id.len() != 64 || !c.pack_id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(CoreError::Manifest(format!(
                    "malformed pack id {:?}",
                    c.pack_id
                )));
            }
        }
        Ok(())
    }

    /// Total stored bytes this manifest references (pre-dedup accounting).
    pub fn referenced_stored_bytes(&self) -> u64 {
        self.chunks.iter().map(|c| c.data_len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        let payload = b"hello sparta".to_vec();
        let h = ChunkHash::of(&payload);
        Manifest {
            format_version: spec::FORMAT_VERSION,
            bucket: "fleet".into(),
            key: "snapshots/a.tar".into(),
            version_id: None,
            logical_size: payload.len() as u64,
            object_hash: h,
            content_type: Some("application/x-tar".into()),
            user_metadata: BTreeMap::from([("x-amz-meta-run".into(), "42".into())]),
            etag: Some("\"abc\"".into()),
            detected_compression: None,
            recipe: None,
            payload_size: payload.len() as u64,
            payload_hash: h,
            chunks: vec![ChunkRef {
                hash: h,
                raw_len: payload.len() as u64,
                pack_id: "ab".repeat(32),
                offset: 61,
                data_len: 10,
                codec: 1,
                dict_id: 0,
            }],
            parent: None,
            minhash: None,
            created_unix_ms: 1_756_000_000_000,
            quarantined: false,
        }
    }

    #[test]
    fn json_roundtrip() {
        let m = sample();
        let bytes = m.to_json().unwrap();
        let back = Manifest::from_json(&bytes).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn validate_rejects_length_mismatch() {
        let mut m = sample();
        m.payload_size += 1;
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_rejects_payload_divergence_without_recipe() {
        let mut m = sample();
        m.payload_hash = ChunkHash::of(b"different");
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_rejects_bad_pack_id() {
        let mut m = sample();
        m.chunks[0].pack_id = "not-hex".into();
        assert!(m.validate().is_err());
    }

    #[test]
    fn empty_object_manifest_is_valid() {
        let mut m = sample();
        m.chunks.clear();
        m.logical_size = 0;
        m.payload_size = 0;
        m.object_hash = ChunkHash::of(b"");
        m.payload_hash = m.object_hash;
        m.validate().unwrap();
    }
}
