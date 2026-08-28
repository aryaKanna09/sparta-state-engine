//! Checkpoint-aware chunking (M5).
//!
//! Model checkpoints have natural internal boundaries — safetensors records
//! per-tensor byte ranges in its JSON header, and PyTorch checkpoints are zip
//! archives with per-entry offsets. Cutting chunks at those boundaries means
//! two adjacent training checkpoints with mostly bit-identical tensors dedup
//! tensor by tensor, instead of depending on FastCDC boundaries happening to
//! land well.
//!
//! Small adjacent regions are merged up to the spec minimum chunk size
//! (deterministically, from the boundary layout alone, so identical structure
//! yields identical merges across checkpoints); regions above the spec
//! maximum are sub-chunked with FastCDC, which restarts cleanly at region
//! starts.

use crate::chunker::{chunk_spans, ChunkSpan};
use crate::hash::ChunkHash;
use crate::spec;

/// Detect checkpoint structure and return interior cut points (sorted,
/// exclusive of 0 and `data.len()`), or None if the payload isn't a
/// recognized checkpoint format.
pub fn detect_boundaries(data: &[u8]) -> Option<Vec<u64>> {
    safetensors_boundaries(data).or_else(|| zip_boundaries(data))
}

/// safetensors: `u64 LE header_len | header JSON | tensor data`.
/// Header JSON maps tensor names to `{"data_offsets": [begin, end], ...}`,
/// offsets relative to the end of the header.
fn safetensors_boundaries(data: &[u8]) -> Option<Vec<u64>> {
    if data.len() < 16 {
        return None;
    }
    let header_len = u64::from_le_bytes(data[..8].try_into().ok()?) as usize;
    // Sanity bounds: headers are small JSON documents.
    if header_len == 0 || header_len > 100 * 1024 * 1024 || 8 + header_len > data.len() {
        return None;
    }
    let header = &data[8..8 + header_len];
    // Cheap structural sniff before paying for a JSON parse.
    if header.first() != Some(&b'{') {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_slice(header).ok()?;
    let obj = parsed.as_object()?;
    let base = (8 + header_len) as u64;
    let mut cuts: Vec<u64> = vec![base];
    let mut saw_tensor = false;
    for (name, v) in obj {
        if name == "__metadata__" {
            continue;
        }
        let offsets = v.get("data_offsets")?.as_array()?;
        let begin = offsets.first()?.as_u64()?;
        saw_tensor = true;
        cuts.push(base + begin);
    }
    if !saw_tensor {
        return None;
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.retain(|&c| c > 0 && c < data.len() as u64);
    Some(cuts)
}

/// PyTorch (and generic) zip: walk the central directory for each entry's
/// local header offset. Entries in torch checkpoints are STORED, so the raw
/// tensor bytes sit uncompressed between boundaries.
fn zip_boundaries(data: &[u8]) -> Option<Vec<u64>> {
    if data.len() < 22 || &data[..4] != b"PK\x03\x04" {
        return None;
    }
    // Find End Of Central Directory: scan the last 64 KiB + 22 bytes.
    const EOCD_MAGIC: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    let scan_start = data.len().saturating_sub(64 * 1024 + 22);
    let tail = &data[scan_start..];
    let eocd_rel = tail.windows(4).rposition(|w| w == EOCD_MAGIC)?;
    let eocd = scan_start + eocd_rel;
    if eocd + 22 > data.len() {
        return None;
    }
    let cd_count = u16::from_le_bytes(data[eocd + 10..eocd + 12].try_into().ok()?) as usize;
    let cd_offset = u32::from_le_bytes(data[eocd + 16..eocd + 20].try_into().ok()?) as usize;
    if cd_offset >= data.len() {
        return None;
    }

    const CDH_MAGIC: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
    let mut cuts = Vec::with_capacity(cd_count + 1);
    let mut p = cd_offset;
    for _ in 0..cd_count {
        if p + 46 > data.len() || data[p..p + 4] != CDH_MAGIC {
            return None;
        }
        let name_len = u16::from_le_bytes(data[p + 28..p + 30].try_into().ok()?) as usize;
        let extra_len = u16::from_le_bytes(data[p + 30..p + 32].try_into().ok()?) as usize;
        let comment_len = u16::from_le_bytes(data[p + 32..p + 34].try_into().ok()?) as usize;
        let local_offset = u32::from_le_bytes(data[p + 42..p + 46].try_into().ok()?) as u64;
        cuts.push(local_offset);
        p += 46 + name_len + extra_len + comment_len;
    }
    // The central directory itself is a boundary too.
    cuts.push(cd_offset as u64);
    cuts.sort_unstable();
    cuts.dedup();
    cuts.retain(|&c| c > 0 && c < data.len() as u64);
    if cuts.is_empty() {
        return None;
    }
    Some(cuts)
}

/// Chunk `data` respecting `boundaries` (sorted interior cut points):
/// consecutive regions smaller than the spec minimum are merged (kept
/// deterministic by layout), regions above the spec maximum are sub-chunked
/// with FastCDC restarted at the region start.
pub fn chunk_spans_with_boundaries(data: &[u8], boundaries: &[u64]) -> Vec<ChunkSpan> {
    let mut spans = Vec::new();
    let mut region_start = 0usize;
    let cut_iter = boundaries
        .iter()
        .map(|&c| c as usize)
        .chain(std::iter::once(data.len()));

    let mut segment_start = 0usize;
    for cut in cut_iter {
        if cut <= region_start || cut > data.len() {
            continue;
        }
        // Accumulate until the merged segment reaches the spec minimum.
        if cut - segment_start < spec::CDC_MIN_SIZE as usize && cut != data.len() {
            region_start = cut;
            continue;
        }
        let segment = &data[segment_start..cut];
        if segment.len() <= spec::CDC_MAX_SIZE as usize {
            if !segment.is_empty() {
                spans.push(ChunkSpan {
                    offset: segment_start,
                    length: segment.len(),
                    hash: ChunkHash::of(segment),
                });
            }
        } else {
            for s in chunk_spans(segment) {
                spans.push(ChunkSpan {
                    offset: segment_start + s.offset,
                    length: s.length,
                    hash: s.hash,
                });
            }
        }
        segment_start = cut;
        region_start = cut;
        if cut == data.len() {
            break;
        }
    }
    // Tail after the last boundary (when the final cut came from a boundary
    // shorter than the data).
    if segment_start < data.len() {
        let segment = &data[segment_start..];
        if segment.len() <= spec::CDC_MAX_SIZE as usize {
            spans.push(ChunkSpan {
                offset: segment_start,
                length: segment.len(),
                hash: ChunkHash::of(segment),
            });
        } else {
            for s in chunk_spans(segment) {
                spans.push(ChunkSpan {
                    offset: segment_start + s.offset,
                    length: s.length,
                    hash: s.hash,
                });
            }
        }
    }
    spans
}

/// Convenience: boundary-aware chunking with generic CDC fallback.
pub fn chunk_checkpoint_aware(data: &[u8]) -> Vec<ChunkSpan> {
    match detect_boundaries(data) {
        Some(b) => chunk_spans_with_boundaries(data, &b),
        None => chunk_spans(data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal safetensors builder for tests.
    pub fn build_safetensors(tensors: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut header = String::from("{");
        let mut offset = 0usize;
        for (i, (name, data)) in tensors.iter().enumerate() {
            if i > 0 {
                header.push(',');
            }
            header.push_str(&format!(
                r#""{name}":{{"dtype":"F32","shape":[{}],"data_offsets":[{offset},{}]}}"#,
                data.len() / 4,
                offset + data.len()
            ));
            offset += data.len();
        }
        header.push('}');
        let mut out = Vec::new();
        out.extend_from_slice(&(header.len() as u64).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        for (_, data) in tensors {
            out.extend_from_slice(data);
        }
        out
    }

    fn tensor(seed: u64, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut s = seed | 1;
        while out.len() < len {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            out.extend_from_slice(&s.to_le_bytes());
        }
        out.truncate(len);
        out
    }

    #[test]
    fn safetensors_detected_and_bounded() {
        let ckpt = build_safetensors(&[
            ("layer.0.weight", tensor(1, 700_000)),
            ("layer.0.bias", tensor(2, 400_000)),
            ("layer.1.weight", tensor(3, 900_000)),
        ]);
        let bounds = detect_boundaries(&ckpt).expect("safetensors detected");
        assert!(bounds.len() >= 3);
        let spans = chunk_spans_with_boundaries(&ckpt, &bounds);
        // Spans must tile the file.
        let mut pos = 0;
        for s in &spans {
            assert_eq!(s.offset, pos);
            pos += s.length;
        }
        assert_eq!(pos, ckpt.len());
    }

    #[test]
    fn not_a_checkpoint_returns_none() {
        assert!(detect_boundaries(b"just some plain bytes, nothing structured").is_none());
        assert!(detect_boundaries(&tensor(9, 100_000)).is_none());
        // A truncated safetensors header must not panic or false-positive.
        let mut bogus = vec![0u8; 16];
        bogus[0] = 200; // header_len larger than the file
        assert!(detect_boundaries(&bogus).is_none());
    }

    #[test]
    fn adjacent_checkpoints_share_unchanged_tensors() {
        // The M5 acceptance shape: two checkpoints, 98% bit-identical tensors.
        // 50 tensors of 512 KiB; one differs => 2% changed.
        let mut tensors_a: Vec<(String, Vec<u8>)> = (0..50)
            .map(|i| (format!("t{i:02}"), tensor(1000 + i, 512 * 1024)))
            .collect();
        let named_a: Vec<(&str, Vec<u8>)> = tensors_a
            .iter()
            .map(|(n, d)| (n.as_str(), d.clone()))
            .collect();
        let a = build_safetensors(&named_a);

        tensors_a[25].1 = tensor(9999, 512 * 1024); // the changed tensor
        let named_b: Vec<(&str, Vec<u8>)> = tensors_a
            .iter()
            .map(|(n, d)| (n.as_str(), d.clone()))
            .collect();
        let b = build_safetensors(&named_b);

        let spans_a = chunk_checkpoint_aware(&a);
        let spans_b = chunk_checkpoint_aware(&b);
        let set_a: std::collections::HashSet<_> = spans_a.iter().map(|s| s.hash).collect();
        let delta: u64 = spans_b
            .iter()
            .filter(|s| !set_a.contains(&s.hash))
            .map(|s| s.length as u64)
            .sum();
        let frac = delta as f64 / b.len() as f64;
        assert!(
            frac < 0.05,
            "delta stored must be <5% of file size, got {:.2}% ({} of {})",
            frac * 100.0,
            delta,
            b.len()
        );
    }

    #[test]
    fn zip_boundaries_detected() {
        // Hand-build a tiny STORED zip with two entries.
        fn stored_entry(name: &str, data: &[u8]) -> (Vec<u8>, u32) {
            let mut e = Vec::new();
            e.extend_from_slice(b"PK\x03\x04");
            e.extend_from_slice(&[20, 0, 0, 0, 0, 0]); // version, flags, method=0
            e.extend_from_slice(&[0, 0, 0, 0]); // time/date
            let crc = {
                let mut c = flate2::Crc::new();
                c.update(data);
                c.sum()
            };
            e.extend_from_slice(&crc.to_le_bytes());
            e.extend_from_slice(&(data.len() as u32).to_le_bytes()); // csize
            e.extend_from_slice(&(data.len() as u32).to_le_bytes()); // usize
            e.extend_from_slice(&(name.len() as u16).to_le_bytes());
            e.extend_from_slice(&0u16.to_le_bytes()); // extra len
            e.extend_from_slice(name.as_bytes());
            e.extend_from_slice(data);
            (e, crc)
        }
        fn central_entry(name: &str, data_len: u32, crc: u32, offset: u32) -> Vec<u8> {
            let mut c = Vec::new();
            c.extend_from_slice(b"PK\x01\x02");
            c.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0]);
            c.extend_from_slice(&[0, 0, 0, 0]); // time/date
            c.extend_from_slice(&crc.to_le_bytes());
            c.extend_from_slice(&data_len.to_le_bytes());
            c.extend_from_slice(&data_len.to_le_bytes());
            c.extend_from_slice(&(name.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0u8; 12]); // extra/comment/disk/attrs
            c.extend_from_slice(&offset.to_le_bytes());
            c.extend_from_slice(name.as_bytes());
            c
        }
        let d1 = tensor(1, 400_000);
        let d2 = tensor(2, 300_000);
        let (e1, crc1) = stored_entry("archive/data/0", &d1);
        let (e2, crc2) = stored_entry("archive/data/1", &d2);
        let off2 = e1.len() as u32;
        let cd_off = (e1.len() + e2.len()) as u32;
        let c1 = central_entry("archive/data/0", d1.len() as u32, crc1, 0);
        let c2 = central_entry("archive/data/1", d2.len() as u32, crc2, off2);
        let cd_len = (c1.len() + c2.len()) as u32;
        let mut zip = Vec::new();
        zip.extend_from_slice(&e1);
        zip.extend_from_slice(&e2);
        zip.extend_from_slice(&c1);
        zip.extend_from_slice(&c2);
        zip.extend_from_slice(b"PK\x05\x06");
        zip.extend_from_slice(&[0, 0, 0, 0]); // disk numbers
        zip.extend_from_slice(&2u16.to_le_bytes());
        zip.extend_from_slice(&2u16.to_le_bytes());
        zip.extend_from_slice(&cd_len.to_le_bytes());
        zip.extend_from_slice(&cd_off.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // comment len

        let bounds = detect_boundaries(&zip).expect("zip detected");
        assert!(bounds.contains(&(off2 as u64)), "{bounds:?}");
        assert!(bounds.contains(&(cd_off as u64)));
        let spans = chunk_spans_with_boundaries(&zip, &bounds);
        let mut pos = 0;
        for s in &spans {
            assert_eq!(s.offset, pos);
            pos += s.length;
        }
        assert_eq!(pos, zip.len());
    }

    #[test]
    fn tiny_tensors_are_merged() {
        // 4000 tiny tensors must not become 4000 chunks.
        let named: Vec<(String, Vec<u8>)> = (0..4000)
            .map(|i| (format!("t{i:04}"), tensor(i, 256)))
            .collect();
        let refs: Vec<(&str, Vec<u8>)> =
            named.iter().map(|(n, d)| (n.as_str(), d.clone())).collect();
        let ckpt = build_safetensors(&refs);
        let spans = chunk_checkpoint_aware(&ckpt);
        assert!(
            spans.len() < 40,
            "tiny tensors must merge into few chunks, got {}",
            spans.len()
        );
        let total: usize = spans.iter().map(|s| s.length).sum();
        assert_eq!(total, ckpt.len());
    }
}
