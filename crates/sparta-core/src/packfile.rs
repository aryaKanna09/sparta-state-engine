//! Packfile format v1.
//!
//! Chunks are appended into 32–64 MB packs in manifest order. Each chunk is
//! individually zstd-compressed (spec level 3) so any single chunk is
//! retrievable with one ranged GET. Packs are self-describing: every record
//! carries its own header, and a full index is embedded at the tail, so
//! `rebuild-index` can reconstruct all metadata from the bucket alone.
//!
//! Layout:
//! ```text
//! magic            b"SPRTPCK1"                       (8 bytes)
//! records          repeated:
//!     b"CHNK"                                        (4)
//!     chunk hash                                     (32)
//!     codec u8   (0 = raw, 1 = zstd, 2 = zstd+dict)  (1)
//!     dict id u64 LE  (0 when codec != 2)            (8)
//!     raw len u64 LE                                 (8)
//!     data len u64 LE                                (8)
//!     data                                           (data len)
//! index            b"SPRTIDX1" (8), count u32 LE (4), then per entry:
//!     chunk hash (32), offset u64 (8), data_len u64 (8),
//!     raw_len u64 (8), codec u8 (1), dict_id u64 (8)   = 65 bytes
//! footer           index offset u64 LE (8), b"SPRTPKF1" (8)
//! ```
//! `offset` in an index entry points at the chunk *data* (past the record
//! header) for direct ranged reads.

use std::collections::HashMap;
use std::io::Read;

use serde::{Deserialize, Serialize};

use crate::hash::ChunkHash;
use crate::{spec, CoreError};

pub const PACK_MAGIC: &[u8; 8] = b"SPRTPCK1";
pub const INDEX_MAGIC: &[u8; 8] = b"SPRTIDX1";
pub const FOOTER_MAGIC: &[u8; 8] = b"SPRTPKF1";
pub const FOOTER_LEN: u64 = 16;

pub const CODEC_RAW: u8 = 0;
pub const CODEC_ZSTD: u8 = 1;
pub const CODEC_ZSTD_DICT: u8 = 2;

const RECORD_HEADER_LEN: usize = 4 + 32 + 1 + 8 + 8 + 8;
const INDEX_ENTRY_LEN: usize = 32 + 8 + 8 + 8 + 1 + 8;

/// Location and shape of one chunk inside a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackEntry {
    pub hash: ChunkHash,
    /// Byte offset of the chunk data within the pack object.
    pub offset: u64,
    /// Stored (possibly compressed) length.
    pub data_len: u64,
    /// Decompressed length.
    pub raw_len: u64,
    pub codec: u8,
    /// Dictionary id for `CODEC_ZSTD_DICT`; 0 otherwise.
    pub dict_id: u64,
}

/// A sealed, immutable pack ready to be written to the bucket.
pub struct SealedPack {
    /// BLAKE3 hex of the full pack bytes; used as the object name.
    pub pack_id: String,
    pub bytes: Vec<u8>,
    pub entries: Vec<PackEntry>,
}

/// Accumulates chunks into a pack. Deduplicates within itself: adding the same
/// hash twice returns the original entry without growing the pack.
pub struct PackBuilder {
    buf: Vec<u8>,
    entries: Vec<PackEntry>,
    by_hash: HashMap<ChunkHash, usize>,
    zstd_level: i32,
}

impl PackBuilder {
    pub fn new() -> Self {
        Self::with_level(spec::PACK_ZSTD_LEVEL)
    }

    pub fn with_level(zstd_level: i32) -> Self {
        let mut buf = Vec::with_capacity(spec::PACK_TARGET_BYTES as usize / 8);
        buf.extend_from_slice(PACK_MAGIC);
        Self {
            buf,
            entries: Vec::new(),
            by_hash: HashMap::new(),
            zstd_level,
        }
    }

    /// Current encoded size in bytes (records only; index/footer added at seal).
    pub fn len(&self) -> u64 {
        self.buf.len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn chunk_count(&self) -> usize {
        self.entries.len()
    }

    /// True once the pack has reached the target size and should be sealed.
    pub fn is_full(&self) -> bool {
        self.len() >= spec::PACK_TARGET_BYTES
    }

    /// True if adding `raw_len` more bytes could exceed the hard ceiling.
    pub fn would_overflow(&self, raw_len: u64) -> bool {
        self.len() + raw_len + RECORD_HEADER_LEN as u64 > spec::PACK_MAX_BYTES
    }

    /// Add a chunk (compressing it), returning its entry.
    pub fn add_chunk(&mut self, hash: ChunkHash, raw: &[u8]) -> Result<PackEntry, CoreError> {
        if let Some(&i) = self.by_hash.get(&hash) {
            return Ok(self.entries[i]);
        }
        let compressed = zstd::stream::encode_all(raw, self.zstd_level)
            .map_err(|e| CoreError::Compression(format!("pack chunk compress: {e}")))?;
        // Store raw when compression doesn't help (already-compressed data).
        let (codec, data) = if compressed.len() < raw.len() {
            (CODEC_ZSTD, compressed.as_slice())
        } else {
            (CODEC_RAW, raw)
        };
        self.add_encoded(hash, codec, 0, raw.len() as u64, data)
    }

    /// Add pre-encoded chunk data (used by the M5 dictionary path).
    pub fn add_encoded(
        &mut self,
        hash: ChunkHash,
        codec: u8,
        dict_id: u64,
        raw_len: u64,
        data: &[u8],
    ) -> Result<PackEntry, CoreError> {
        if let Some(&i) = self.by_hash.get(&hash) {
            return Ok(self.entries[i]);
        }
        self.buf.extend_from_slice(b"CHNK");
        self.buf.extend_from_slice(hash.as_bytes());
        self.buf.push(codec);
        self.buf.extend_from_slice(&dict_id.to_le_bytes());
        self.buf.extend_from_slice(&raw_len.to_le_bytes());
        self.buf
            .extend_from_slice(&(data.len() as u64).to_le_bytes());
        let offset = self.buf.len() as u64;
        self.buf.extend_from_slice(data);
        let entry = PackEntry {
            hash,
            offset,
            data_len: data.len() as u64,
            raw_len,
            codec,
            dict_id,
        };
        self.by_hash.insert(hash, self.entries.len());
        self.entries.push(entry);
        Ok(entry)
    }

    /// Finalize: append the index and footer and compute the pack id.
    pub fn seal(mut self) -> SealedPack {
        let index_offset = self.buf.len() as u64;
        self.buf.extend_from_slice(INDEX_MAGIC);
        self.buf
            .extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for e in &self.entries {
            self.buf.extend_from_slice(e.hash.as_bytes());
            self.buf.extend_from_slice(&e.offset.to_le_bytes());
            self.buf.extend_from_slice(&e.data_len.to_le_bytes());
            self.buf.extend_from_slice(&e.raw_len.to_le_bytes());
            self.buf.push(e.codec);
            self.buf.extend_from_slice(&e.dict_id.to_le_bytes());
        }
        self.buf.extend_from_slice(&index_offset.to_le_bytes());
        self.buf.extend_from_slice(FOOTER_MAGIC);
        let pack_id = ChunkHash::of(&self.buf).to_hex();
        SealedPack {
            pack_id,
            bytes: self.buf,
            entries: self.entries,
        }
    }
}

impl Default for PackBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Byte range of the footer for a pack of `pack_len` bytes (for a ranged GET).
pub fn footer_range(pack_len: u64) -> (u64, u64) {
    (pack_len.saturating_sub(FOOTER_LEN), pack_len)
}

/// Parse the 16-byte footer, returning the index offset.
pub fn parse_footer(tail: &[u8]) -> Result<u64, CoreError> {
    if tail.len() != FOOTER_LEN as usize || &tail[8..] != FOOTER_MAGIC {
        return Err(CoreError::PackFormat("bad pack footer".into()));
    }
    Ok(u64::from_le_bytes(tail[..8].try_into().unwrap()))
}

/// Parse the index section (starting at the index magic).
pub fn parse_index(index_bytes: &[u8]) -> Result<Vec<PackEntry>, CoreError> {
    if index_bytes.len() < 12 || &index_bytes[..8] != INDEX_MAGIC {
        return Err(CoreError::PackFormat("bad pack index magic".into()));
    }
    let count = u32::from_le_bytes(index_bytes[8..12].try_into().unwrap()) as usize;
    let need = 12 + count * INDEX_ENTRY_LEN;
    if index_bytes.len() < need {
        return Err(CoreError::PackFormat(format!(
            "pack index truncated: need {need} bytes, have {}",
            index_bytes.len()
        )));
    }
    let mut entries = Vec::with_capacity(count);
    let mut p = 12;
    for _ in 0..count {
        let hash = ChunkHash(index_bytes[p..p + 32].try_into().unwrap());
        let offset = u64::from_le_bytes(index_bytes[p + 32..p + 40].try_into().unwrap());
        let data_len = u64::from_le_bytes(index_bytes[p + 40..p + 48].try_into().unwrap());
        let raw_len = u64::from_le_bytes(index_bytes[p + 48..p + 56].try_into().unwrap());
        let codec = index_bytes[p + 56];
        let dict_id = u64::from_le_bytes(index_bytes[p + 57..p + 65].try_into().unwrap());
        entries.push(PackEntry {
            hash,
            offset,
            data_len,
            raw_len,
            codec,
            dict_id,
        });
        p += INDEX_ENTRY_LEN;
    }
    Ok(entries)
}

/// Read the embedded index from a fully materialized pack.
pub fn read_index(pack_bytes: &[u8]) -> Result<Vec<PackEntry>, CoreError> {
    if pack_bytes.len() < 8 + FOOTER_LEN as usize || &pack_bytes[..8] != PACK_MAGIC {
        return Err(CoreError::PackFormat("bad pack magic".into()));
    }
    let (start, _) = footer_range(pack_bytes.len() as u64);
    let index_offset = parse_footer(&pack_bytes[start as usize..])? as usize;
    if index_offset >= start as usize {
        return Err(CoreError::PackFormat("index offset out of range".into()));
    }
    parse_index(&pack_bytes[index_offset..start as usize])
}

/// Recovery scan: walk records front-to-back, ignoring the index entirely.
/// Used when a pack's tail is damaged; also cross-checks the index in tests.
pub fn scan_records(pack_bytes: &[u8]) -> Result<Vec<PackEntry>, CoreError> {
    if pack_bytes.len() < 8 || &pack_bytes[..8] != PACK_MAGIC {
        return Err(CoreError::PackFormat("bad pack magic".into()));
    }
    let mut entries = Vec::new();
    let mut p = 8usize;
    while p + 4 <= pack_bytes.len() && &pack_bytes[p..p + 4] == b"CHNK" {
        if p + RECORD_HEADER_LEN > pack_bytes.len() {
            return Err(CoreError::PackFormat("truncated record header".into()));
        }
        let hash = ChunkHash(pack_bytes[p + 4..p + 36].try_into().unwrap());
        let codec = pack_bytes[p + 36];
        let dict_id = u64::from_le_bytes(pack_bytes[p + 37..p + 45].try_into().unwrap());
        let raw_len = u64::from_le_bytes(pack_bytes[p + 45..p + 53].try_into().unwrap());
        let data_len = u64::from_le_bytes(pack_bytes[p + 53..p + 61].try_into().unwrap());
        let offset = (p + RECORD_HEADER_LEN) as u64;
        let end = offset
            .checked_add(data_len)
            .ok_or_else(|| CoreError::PackFormat("record length overflow".into()))?;
        if end > pack_bytes.len() as u64 {
            return Err(CoreError::PackFormat("truncated record data".into()));
        }
        entries.push(PackEntry {
            hash,
            offset,
            data_len,
            raw_len,
            codec,
            dict_id,
        });
        p = end as usize;
    }
    Ok(entries)
}

/// Compress a chunk against a raw-content zstd dictionary (parent delta or
/// trained dictionary). Returns the encoded bytes.
pub fn compress_chunk_with_dict(raw: &[u8], dict: &[u8], level: i32) -> Result<Vec<u8>, CoreError> {
    let mut c = zstd::bulk::Compressor::with_dictionary(level, dict)
        .map_err(|e| CoreError::Compression(format!("dict compressor: {e}")))?;
    c.compress(raw)
        .map_err(|e| CoreError::Compression(format!("dict compress: {e}")))
}

/// Decode chunk data fetched from a pack and verify its BLAKE3 address.
pub fn decode_chunk(
    entry: &PackEntry,
    data: &[u8],
    dict: Option<&[u8]>,
) -> Result<Vec<u8>, CoreError> {
    if data.len() as u64 != entry.data_len {
        return Err(CoreError::PackFormat(format!(
            "chunk data length mismatch: expected {}, got {}",
            entry.data_len,
            data.len()
        )));
    }
    let raw = match entry.codec {
        CODEC_RAW => data.to_vec(),
        CODEC_ZSTD => {
            let mut out = Vec::with_capacity(entry.raw_len as usize);
            zstd::stream::Decoder::new(data)
                .and_then(|mut d| d.read_to_end(&mut out))
                .map_err(|e| CoreError::Compression(format!("chunk decompress: {e}")))?;
            out
        }
        CODEC_ZSTD_DICT => {
            let dict = dict.ok_or_else(|| {
                CoreError::PackFormat(format!(
                    "chunk requires dictionary {} but none provided",
                    entry.dict_id
                ))
            })?;
            let dec = zstd::dict::DecoderDictionary::copy(dict);
            let mut out = Vec::with_capacity(entry.raw_len as usize);
            zstd::stream::Decoder::with_prepared_dictionary(data, &dec)
                .and_then(|mut d| d.read_to_end(&mut out))
                .map_err(|e| CoreError::Compression(format!("dict chunk decompress: {e}")))?;
            out
        }
        other => {
            return Err(CoreError::PackFormat(format!("unknown codec {other}")));
        }
    };
    let actual = ChunkHash::of(&raw);
    if actual != entry.hash {
        return Err(CoreError::ChunkHashMismatch {
            expected: entry.hash.to_hex(),
            actual: actual.to_hex(),
        });
    }
    if raw.len() as u64 != entry.raw_len {
        return Err(CoreError::PackFormat(format!(
            "raw length mismatch: expected {}, got {}",
            entry.raw_len,
            raw.len()
        )));
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(seed: u8, len: usize) -> (ChunkHash, Vec<u8>) {
        let data: Vec<u8> = (0..len)
            .map(|i| seed.wrapping_add((i / 32) as u8))
            .collect();
        (ChunkHash::of(&data), data)
    }

    #[test]
    fn build_seal_read_roundtrip() {
        let mut b = PackBuilder::new();
        let mut originals = Vec::new();
        for s in 0..10u8 {
            let (h, data) = chunk(s, 300_000 + s as usize * 1000);
            b.add_chunk(h, &data).unwrap();
            originals.push((h, data));
        }
        let sealed = b.seal();
        assert_eq!(sealed.pack_id.len(), 64);

        let idx = read_index(&sealed.bytes).unwrap();
        assert_eq!(idx.len(), 10);
        assert_eq!(idx, sealed.entries);

        for (entry, (h, data)) in idx.iter().zip(&originals) {
            assert_eq!(&entry.hash, h);
            let stored =
                &sealed.bytes[entry.offset as usize..(entry.offset + entry.data_len) as usize];
            let raw = decode_chunk(entry, stored, None).unwrap();
            assert_eq!(&raw, data);
        }
    }

    #[test]
    fn incompressible_chunks_stored_raw() {
        let mut data = vec![0u8; 200_000];
        let mut s = 0x12345678u64;
        for b in data.iter_mut() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *b = s as u8;
        }
        let h = ChunkHash::of(&data);
        let mut b = PackBuilder::new();
        let e = b.add_chunk(h, &data).unwrap();
        assert_eq!(e.codec, CODEC_RAW);
        assert_eq!(e.data_len, e.raw_len);
    }

    #[test]
    fn within_pack_dedup() {
        let (h, data) = chunk(1, 100_000);
        let mut b = PackBuilder::new();
        let e1 = b.add_chunk(h, &data).unwrap();
        let len_after_one = b.len();
        let e2 = b.add_chunk(h, &data).unwrap();
        assert_eq!(e1, e2);
        assert_eq!(b.len(), len_after_one, "duplicate add must not grow pack");
        assert_eq!(b.chunk_count(), 1);
    }

    #[test]
    fn scan_matches_index() {
        let mut b = PackBuilder::new();
        for s in 0..7u8 {
            let (h, data) = chunk(s, 150_000);
            b.add_chunk(h, &data).unwrap();
        }
        let sealed = b.seal();
        let scanned = scan_records(&sealed.bytes).unwrap();
        assert_eq!(scanned, read_index(&sealed.bytes).unwrap());
    }

    #[test]
    fn corrupt_footer_detected() {
        let mut b = PackBuilder::new();
        let (h, data) = chunk(3, 50_000);
        b.add_chunk(h, &data).unwrap();
        let mut bytes = b.seal().bytes;
        let n = bytes.len();
        bytes[n - 1] ^= 0xff;
        assert!(read_index(&bytes).is_err());
        // Recovery scan still works because records are self-describing.
        assert_eq!(scan_records(&bytes).unwrap().len(), 1);
    }

    #[test]
    fn corrupt_chunk_data_detected_on_decode() {
        let mut b = PackBuilder::new();
        let (h, data) = chunk(9, 80_000);
        b.add_chunk(h, &data).unwrap();
        let sealed = b.seal();
        let e = sealed.entries[0];
        let mut stored = sealed.bytes[e.offset as usize..(e.offset + e.data_len) as usize].to_vec();
        stored[10] ^= 0x01;
        assert!(decode_chunk(&e, &stored, None).is_err());
    }

    #[test]
    fn dict_compressed_chunk_roundtrips() {
        // Parent-delta shape: child differs from parent in a small hotspot.
        let parent: Vec<u8> = (0..600_000u32).map(|i| (i % 251) as u8).collect();
        let mut child = parent.clone();
        for b in &mut child[100_000..104_096] {
            *b ^= 0x5a;
        }
        let h = ChunkHash::of(&child);
        let delta = compress_chunk_with_dict(&child, &parent, 3).unwrap();
        let plain = zstd::stream::encode_all(child.as_slice(), 3).unwrap();
        assert!(
            delta.len() < plain.len(),
            "parent dict must beat plain zstd here ({} vs {})",
            delta.len(),
            plain.len()
        );
        let mut b = PackBuilder::new();
        let e = b
            .add_encoded(h, CODEC_ZSTD_DICT, 0, child.len() as u64, &delta)
            .unwrap();
        let sealed = b.seal();
        let stored = &sealed.bytes[e.offset as usize..(e.offset + e.data_len) as usize];
        let restored = decode_chunk(&e, stored, Some(&parent)).unwrap();
        assert_eq!(restored, child);
        // Wrong dictionary must fail hash verification, not return bad bytes.
        assert!(decode_chunk(&e, stored, Some(&child[..1000])).is_err());
    }

    #[test]
    fn pack_id_is_content_hash() {
        let mk = || {
            let mut b = PackBuilder::new();
            let (h, data) = chunk(5, 60_000);
            b.add_chunk(h, &data).unwrap();
            b.seal()
        };
        assert_eq!(mk().pack_id, mk().pack_id);
    }
}
