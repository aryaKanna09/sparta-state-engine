//! Compression detection and normalization.
//!
//! Normalization decompresses recognized payloads and chunks the *raw* bytes,
//! recording a recompression recipe. A recipe is only ever emitted after a full
//! byte-exact verification: we recompress the raw bytes with the candidate
//! recipe and require the output to equal the original stream, byte for byte.
//! Anything that fails verification is stored **opaque**: original bytes,
//! chunked as-is, returned untouched. Correctness beats ratio.
//!
//! MVP scope (see DECISIONS.md D-003): gzip and zstd are normalized; xz and zip
//! are detected but always opaque.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// Compression formats we can recognize from magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressionKind {
    Gzip,
    Zstd,
    Xz,
    Zip,
}

impl CompressionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CompressionKind::Gzip => "gzip",
            CompressionKind::Zstd => "zstd",
            CompressionKind::Xz => "xz",
            CompressionKind::Zip => "zip",
        }
    }
}

/// Detect compression by magic bytes alone (never by file extension).
pub fn detect(data: &[u8]) -> Option<CompressionKind> {
    if data.len() < 4 {
        return None;
    }
    match data {
        [0x1f, 0x8b, ..] => Some(CompressionKind::Gzip),
        [0x28, 0xb5, 0x2f, 0xfd, ..] => Some(CompressionKind::Zstd),
        [0xfd, b'7', b'z', b'X', b'Z', 0x00, ..] => Some(CompressionKind::Xz),
        [b'P', b'K', 0x03, 0x04, ..]
        | [b'P', b'K', 0x05, 0x06, ..]
        | [b'P', b'K', 0x07, 0x08, ..] => Some(CompressionKind::Zip),
        _ => None,
    }
}

/// Everything needed to reproduce the original compressed byte stream exactly
/// from the raw payload. Only ever stored after byte-exact verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "algo", rename_all = "lowercase")]
pub enum RecompressionRecipe {
    /// gzip: verbatim original header bytes + deflate level. The trailer
    /// (CRC32 + ISIZE) is deterministic from the raw payload.
    Gzip {
        #[serde(with = "hex_vec")]
        header: Vec<u8>,
        level: u32,
    },
    /// zstd: single frame, standard parameters.
    Zstd {
        level: i32,
        checksum: bool,
        content_size: bool,
    },
}

mod hex_vec {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Result of running the sniff/normalize pipeline on an object.
#[derive(Debug)]
pub enum SniffOutcome {
    /// Chunk the original bytes as-is. `detected` records a recognized format
    /// that we chose not to (or failed to) normalize.
    Opaque { detected: Option<CompressionKind> },
    /// Verified normalization: chunk `raw`, store `recipe` in the manifest.
    Normalized {
        detected: CompressionKind,
        raw: Vec<u8>,
        recipe: RecompressionRecipe,
    },
}

/// Sniff and (when possible) normalize a payload.
///
/// `max_raw_bytes` bounds the decompressed size we are willing to materialize;
/// larger payloads (or decompression bombs) fall back to opaque.
/// With `enabled == false` the function only detects, never normalizes.
pub fn normalize(original: &[u8], enabled: bool, max_raw_bytes: u64) -> SniffOutcome {
    let Some(kind) = detect(original) else {
        return SniffOutcome::Opaque { detected: None };
    };
    if !enabled {
        return SniffOutcome::Opaque {
            detected: Some(kind),
        };
    }
    let attempt = match kind {
        CompressionKind::Gzip => normalize_gzip(original, max_raw_bytes),
        CompressionKind::Zstd => normalize_zstd(original, max_raw_bytes),
        // xz/zip: detected but opaque in the MVP (DECISIONS.md D-003).
        CompressionKind::Xz | CompressionKind::Zip => None,
    };
    match attempt {
        Some((raw, recipe)) => SniffOutcome::Normalized {
            detected: kind,
            raw,
            recipe,
        },
        None => SniffOutcome::Opaque {
            detected: Some(kind),
        },
    }
}

/// Reproduce the original compressed bytes from the raw payload and a recipe.
/// The gateway must still verify the object-level BLAKE3 before serving.
pub fn reproduce(recipe: &RecompressionRecipe, raw: &[u8]) -> Result<Vec<u8>, CoreError> {
    match recipe {
        RecompressionRecipe::Gzip { header, level } => {
            let mut out = Vec::with_capacity(header.len() + raw.len() / 2 + 64);
            out.extend_from_slice(header);
            let mut enc =
                flate2::write::DeflateEncoder::new(&mut out, flate2::Compression::new(*level));
            enc.write_all(raw)
                .and_then(|_| enc.try_finish())
                .map_err(|e| CoreError::Compression(format!("gzip reproduce: {e}")))?;
            drop(enc);
            out.extend_from_slice(&gzip_trailer(raw));
            Ok(out)
        }
        RecompressionRecipe::Zstd {
            level,
            checksum,
            content_size,
        } => {
            let mut out = Vec::with_capacity(raw.len() / 2 + 64);
            write_zstd(&mut out, raw, *level, *checksum, *content_size)
                .map_err(|e| CoreError::Compression(format!("zstd reproduce: {e}")))?;
            Ok(out)
        }
    }
}

// ---------------------------------------------------------------------------
// gzip
// ---------------------------------------------------------------------------

const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;

/// Parse a gzip member header, returning its total length in bytes.
fn gzip_header_len(data: &[u8]) -> Option<usize> {
    if data.len() < 10 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
        return None; // CM must be 8 (deflate)
    }
    let flg = data[3];
    let mut pos = 10usize;
    if flg & FEXTRA != 0 {
        let xlen = u16::from_le_bytes([*data.get(pos)?, *data.get(pos + 1)?]) as usize;
        pos = pos.checked_add(2 + xlen)?;
    }
    if flg & FNAME != 0 {
        let rel = data.get(pos..)?.iter().position(|&b| b == 0)?;
        pos += rel + 1;
    }
    if flg & FCOMMENT != 0 {
        let rel = data.get(pos..)?.iter().position(|&b| b == 0)?;
        pos += rel + 1;
    }
    if flg & FHCRC != 0 {
        pos = pos.checked_add(2)?;
    }
    if pos > data.len() {
        return None;
    }
    Some(pos)
}

fn gzip_trailer(raw: &[u8]) -> [u8; 8] {
    let mut crc = flate2::Crc::new();
    crc.update(raw);
    let mut t = [0u8; 8];
    t[..4].copy_from_slice(&crc.sum().to_le_bytes());
    t[4..].copy_from_slice(&(raw.len() as u32).to_le_bytes());
    t
}

fn normalize_gzip(original: &[u8], max_raw_bytes: u64) -> Option<(Vec<u8>, RecompressionRecipe)> {
    let hlen = gzip_header_len(original)?;
    if original.len() < hlen + 8 {
        return None;
    }
    let deflate_body = &original[hlen..original.len() - 8];
    let trailer = &original[original.len() - 8..];

    // Decompress with a bomb guard: read at most max+1 bytes.
    let mut raw = Vec::new();
    let mut dec = flate2::read::DeflateDecoder::new(deflate_body).take(max_raw_bytes + 1);
    dec.read_to_end(&mut raw).ok()?;
    if raw.len() as u64 > max_raw_bytes {
        return None;
    }
    // A multi-member stream leaves deflate bytes unconsumed relative to a
    // single member; the trailer check below catches integrity, and the
    // byte-exact verification catches framing, so no special-casing needed.
    if gzip_trailer(&raw) != *trailer {
        return None;
    }

    // Probe deflate levels, XFL-hinted first. XFL=2 => max compression (9),
    // XFL=4 => fastest (1); default hint 6.
    let xfl = original[8];
    let first = match xfl {
        2 => 9u32,
        4 => 1u32,
        _ => 6u32,
    };
    let mut levels = vec![first];
    levels.extend((1..=9u32).filter(|l| *l != first));

    let header = original[..hlen].to_vec();
    for level in levels {
        if verify_gzip(original, &header, &raw, level) {
            return Some((raw, RecompressionRecipe::Gzip { header, level }));
        }
    }
    None
}

fn verify_gzip(original: &[u8], header: &[u8], raw: &[u8], level: u32) -> bool {
    let expected_deflate = &original[header.len()..original.len() - 8];
    let mut cmp = CompareWriter::new(expected_deflate);
    let mut enc = flate2::write::DeflateEncoder::new(&mut cmp, flate2::Compression::new(level));
    if enc.write_all(raw).is_err() || enc.try_finish().is_err() {
        return false;
    }
    drop(enc);
    cmp.fully_matched()
    // Header is copied verbatim and the trailer was validated during
    // decompression, so a matching deflate body implies byte identity.
}

// ---------------------------------------------------------------------------
// zstd
// ---------------------------------------------------------------------------

fn write_zstd<W: Write>(
    out: W,
    raw: &[u8],
    level: i32,
    checksum: bool,
    content_size: bool,
) -> io::Result<()> {
    let mut enc = zstd::stream::Encoder::new(out, level)?;
    enc.include_checksum(checksum)?;
    if content_size {
        // Pledging the size both writes the content-size header field and lets
        // the encoder clamp its window to the payload — the file-input path of
        // the zstd CLI. Not pledging matches streaming encoders (encode_all).
        enc.include_contentsize(true)?;
        enc.set_pledged_src_size(Some(raw.len() as u64))?;
    } else {
        enc.include_contentsize(false)?;
    }
    enc.write_all(raw)?;
    enc.finish()?;
    Ok(())
}

fn normalize_zstd(original: &[u8], max_raw_bytes: u64) -> Option<(Vec<u8>, RecompressionRecipe)> {
    let mut raw = Vec::new();
    let mut dec = zstd::stream::read::Decoder::new(original)
        .ok()?
        .take(max_raw_bytes + 1);
    dec.read_to_end(&mut raw).ok()?;
    if raw.len() as u64 > max_raw_bytes {
        return None;
    }

    // The frame header does not record the level, so probe. Flag combinations
    // change the first bytes of the frame, so wrong combinations abort almost
    // immediately via CompareWriter; wrong levels typically diverge early too.
    let mut levels = vec![3i32];
    levels.extend((1..=19i32).filter(|l| *l != 3));
    for content_size in [true, false] {
        for checksum in [false, true] {
            for &level in &levels {
                let mut cmp = CompareWriter::new(original);
                if write_zstd(&mut cmp, &raw, level, checksum, content_size).is_ok()
                    && cmp.fully_matched()
                {
                    return Some((
                        raw,
                        RecompressionRecipe::Zstd {
                            level,
                            checksum,
                            content_size,
                        },
                    ));
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Byte-exact verification plumbing
// ---------------------------------------------------------------------------

/// A `Write` sink that compares incoming bytes against an expected slice and
/// errors on the first divergence, so failed recompression probes abort after
/// compressing only a small prefix instead of the whole payload.
struct CompareWriter<'a> {
    expected: &'a [u8],
    pos: usize,
    diverged: bool,
}

impl<'a> CompareWriter<'a> {
    fn new(expected: &'a [u8]) -> Self {
        Self {
            expected,
            pos: 0,
            diverged: false,
        }
    }

    fn fully_matched(&self) -> bool {
        !self.diverged && self.pos == self.expected.len()
    }
}

impl Write for CompareWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let end = self.pos + buf.len();
        if end > self.expected.len() || self.expected[self.pos..end] != *buf {
            self.diverged = true;
            return Err(io::Error::other("recompression diverged from original"));
        }
        self.pos = end;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: u64 = 1 << 30;

    fn compressible(len: usize) -> Vec<u8> {
        // Repetitive but not constant: compresses well at every level.
        (0..len).map(|i| ((i / 64) % 251) as u8).collect()
    }

    #[test]
    fn detects_magic_bytes() {
        assert_eq!(detect(&[0x1f, 0x8b, 8, 0]), Some(CompressionKind::Gzip));
        assert_eq!(
            detect(&[0x28, 0xb5, 0x2f, 0xfd, 0, 0]),
            Some(CompressionKind::Zstd)
        );
        assert_eq!(
            detect(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]),
            Some(CompressionKind::Xz)
        );
        assert_eq!(detect(b"PK\x03\x04rest"), Some(CompressionKind::Zip));
        assert_eq!(detect(b"plain text here"), None);
        assert_eq!(detect(b"\x1f"), None);
    }

    #[test]
    fn gzip_roundtrip_plain() {
        let raw = compressible(2_000_000);
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        enc.write_all(&raw).unwrap();
        let original = enc.finish().unwrap();

        match normalize(&original, true, MAX) {
            SniffOutcome::Normalized {
                detected,
                raw: got_raw,
                recipe,
            } => {
                assert_eq!(detected, CompressionKind::Gzip);
                assert_eq!(got_raw, raw);
                let rebuilt = reproduce(&recipe, &got_raw).unwrap();
                assert_eq!(rebuilt, original, "reproduction must be byte-exact");
            }
            other => panic!("expected normalization, got {other:?}"),
        }
    }

    #[test]
    fn gzip_roundtrip_with_header_fields() {
        let raw = compressible(300_000);
        let mut enc = flate2::GzBuilder::new()
            .filename("snapshot.tar")
            .comment("sparta fixture")
            .mtime(1_700_000_000)
            .write(Vec::new(), flate2::Compression::new(9));
        enc.write_all(&raw).unwrap();
        let original = enc.finish().unwrap();

        match normalize(&original, true, MAX) {
            SniffOutcome::Normalized { recipe, raw: r, .. } => {
                assert_eq!(reproduce(&recipe, &r).unwrap(), original);
                let RecompressionRecipe::Gzip { header, .. } = &recipe else {
                    panic!("wrong recipe kind");
                };
                // Header must be preserved verbatim, including the filename.
                assert!(header.windows(12).any(|w| w == b"snapshot.tar"));
            }
            other => panic!("expected normalization, got {other:?}"),
        }
    }

    #[test]
    fn corrupt_gzip_trailer_goes_opaque() {
        let raw = compressible(100_000);
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        enc.write_all(&raw).unwrap();
        let mut original = enc.finish().unwrap();
        let n = original.len();
        original[n - 2] ^= 0xff; // corrupt ISIZE

        match normalize(&original, true, MAX) {
            SniffOutcome::Opaque { detected } => {
                assert_eq!(detected, Some(CompressionKind::Gzip))
            }
            other => panic!("expected opaque, got {other:?}"),
        }
    }

    #[test]
    fn zstd_roundtrip_various_params() {
        let raw = compressible(1_500_000);
        for (level, checksum, content_size) in [
            (3, false, true),
            (7, true, true),
            (1, false, false),
            (19, true, false),
        ] {
            let mut original = Vec::new();
            write_zstd(&mut original, &raw, level, checksum, content_size).unwrap();

            match normalize(&original, true, MAX) {
                SniffOutcome::Normalized { recipe, raw: r, .. } => {
                    assert_eq!(r, raw);
                    assert_eq!(
                        reproduce(&recipe, &r).unwrap(),
                        original,
                        "params ({level},{checksum},{content_size})"
                    );
                }
                other => panic!(
                    "expected normalization for ({level},{checksum},{content_size}), got {other:?}"
                ),
            }
        }
    }

    #[test]
    fn zstd_foreign_frame_goes_opaque_not_wrong() {
        // A frame our probe can't reproduce (window params from a dictionary
        // trained encoder are out of scope here) must degrade to opaque, never
        // to a wrong recipe. Simulate with a hand-tweaked high-window frame:
        // compress with a level we do probe, then corrupt a content byte so
        // decode fails entirely.
        let raw = compressible(500_000);
        let mut original = Vec::new();
        write_zstd(&mut original, &raw, 3, true, true).unwrap();
        let mid = original.len() / 2;
        original[mid] ^= 0x5a;

        match normalize(&original, true, MAX) {
            SniffOutcome::Opaque { detected } => {
                assert_eq!(detected, Some(CompressionKind::Zstd))
            }
            SniffOutcome::Normalized { recipe, raw: r, .. } => {
                // If it still decoded (checksum caught nothing and deflate
                // happened to parse), the recipe must reproduce the original
                // exactly — that is the actual invariant.
                assert_eq!(reproduce(&recipe, &r).unwrap(), original);
            }
        }
    }

    #[test]
    fn normalization_disabled_detects_only() {
        let raw = compressible(10_000);
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        enc.write_all(&raw).unwrap();
        let original = enc.finish().unwrap();
        match normalize(&original, false, MAX) {
            SniffOutcome::Opaque { detected } => {
                assert_eq!(detected, Some(CompressionKind::Gzip))
            }
            other => panic!("expected opaque, got {other:?}"),
        }
    }

    #[test]
    fn bomb_guard_falls_back_to_opaque() {
        let raw = vec![0u8; 4_000_000];
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(9));
        enc.write_all(&raw).unwrap();
        let original = enc.finish().unwrap();
        match normalize(&original, true, 1_000_000) {
            SniffOutcome::Opaque { detected } => {
                assert_eq!(detected, Some(CompressionKind::Gzip))
            }
            other => panic!("expected opaque under bomb guard, got {other:?}"),
        }
    }

    #[test]
    fn xz_and_zip_are_opaque() {
        let xz = [0xfd, b'7', b'z', b'X', b'Z', 0x00, 1, 2, 3];
        match normalize(&xz, true, MAX) {
            SniffOutcome::Opaque { detected } => assert_eq!(detected, Some(CompressionKind::Xz)),
            other => panic!("{other:?}"),
        }
        let zip = b"PK\x03\x04\x14\x00\x00\x00";
        match normalize(zip, true, MAX) {
            SniffOutcome::Opaque { detected } => assert_eq!(detected, Some(CompressionKind::Zip)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn recipe_serde_roundtrip() {
        let g = RecompressionRecipe::Gzip {
            header: vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255],
            level: 6,
        };
        let z = RecompressionRecipe::Zstd {
            level: 3,
            checksum: true,
            content_size: false,
        };
        for r in [g, z] {
            let s = serde_json::to_string(&r).unwrap();
            let back: RecompressionRecipe = serde_json::from_str(&s).unwrap();
            assert_eq!(back, r);
        }
    }
}
