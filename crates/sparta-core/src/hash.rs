//! BLAKE3 content addressing.

use std::fmt;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::CoreError;

/// A 256-bit BLAKE3 digest identifying a chunk (or any byte string).
///
/// Serialized as lowercase hex in JSON so manifests are grep-able and
/// implementation-portable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkHash(pub [u8; 32]);

impl ChunkHash {
    /// Hash a byte slice.
    pub fn of(data: &[u8]) -> Self {
        Self(*blake3::hash(data).as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self, CoreError> {
        let bytes = hex::decode(s).map_err(|_| CoreError::InvalidHash(s.to_string()))?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CoreError::InvalidHash(s.to_string()))?;
        Ok(Self(arr))
    }

    /// First 8 bytes as a little-endian u64. Used as the MinHash element key;
    /// 64 bits of a BLAKE3 digest is collision-safe at sketch scale.
    pub fn prefix_u64(&self) -> u64 {
        u64::from_le_bytes(self.0[..8].try_into().expect("32-byte digest"))
    }
}

/// Incremental hasher for streaming paths (multipart uploads, GET verification).
pub struct StreamingHash(blake3::Hasher);

impl StreamingHash {
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }

    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    pub fn finalize(&self) -> ChunkHash {
        ChunkHash(*self.0.finalize().as_bytes())
    }
}

impl Default for StreamingHash {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ChunkHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for ChunkHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChunkHash({})", self.to_hex())
    }
}

impl Serialize for ChunkHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for ChunkHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ChunkHash::from_hex(&s).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let h = ChunkHash::of(b"sparta");
        let hex = h.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(ChunkHash::from_hex(&hex).unwrap(), h);
    }

    #[test]
    fn serde_is_hex_string() {
        let h = ChunkHash::of(b"sparta");
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(json, format!("\"{}\"", h.to_hex()));
        let back: ChunkHash = serde_json::from_str(&json).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(ChunkHash::from_hex("zz").is_err());
        assert!(ChunkHash::from_hex("abcd").is_err());
    }

    #[test]
    fn streaming_matches_oneshot() {
        let data = vec![7u8; 1_000_000];
        let mut s = StreamingHash::new();
        for part in data.chunks(4096) {
            s.update(part);
        }
        assert_eq!(s.finalize(), ChunkHash::of(&data));
    }
}
