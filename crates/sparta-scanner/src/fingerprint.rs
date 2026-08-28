//! Per-object fingerprinting: fetch, sniff/normalize, chunk, sketch.

use std::collections::HashMap;
use std::sync::Mutex;

use sparta_core::chunker::chunk_spans;
use sparta_core::compression::{normalize, SniffOutcome};
use sparta_core::hash::ChunkHash;
use sparta_core::minhash::MinHashSketch;
use sparta_core::srf::zstd_stored_estimate;

use crate::types::{ChunkObs, ObjectFingerprint, ObjectMeta};

/// Cross-object cache of stored-size estimates so a chunk seen twice is only
/// compressed once. Shared across worker threads.
#[derive(Default)]
pub struct StoredSizeCache {
    map: Mutex<HashMap<ChunkHash, u64>>,
}

impl StoredSizeCache {
    pub fn estimate(&self, hash: &ChunkHash, data: &[u8]) -> u64 {
        if let Some(v) = self.map.lock().unwrap().get(hash) {
            return *v;
        }
        let est = zstd_stored_estimate(data);
        self.map.lock().unwrap().insert(*hash, est);
        est
    }
}

/// Bound on decompressed size we'll materialize per object during scanning.
const MAX_RAW_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// CPU-side fingerprinting of fetched bytes. Runs in `spawn_blocking`.
#[allow(clippy::too_many_arguments)]
pub fn fingerprint_bytes(
    meta: &ObjectMeta,
    bytes: &[u8],
    content_type: Option<String>,
    prefix_group: String,
    normalization_enabled: bool,
    truncated: bool,
    cache: &StoredSizeCache,
) -> ObjectFingerprint {
    // Truncated windows can't be normalized (the stream is incomplete).
    let outcome = if truncated {
        SniffOutcome::Opaque {
            detected: sparta_core::compression::detect(bytes),
        }
    } else {
        normalize(bytes, normalization_enabled, MAX_RAW_BYTES)
    };

    let (payload, detected, normalized): (&[u8], Option<String>, bool) = match &outcome {
        SniffOutcome::Normalized { raw, detected, .. } => {
            (raw.as_slice(), Some(detected.as_str().to_string()), true)
        }
        SniffOutcome::Opaque { detected } => {
            (bytes, detected.map(|d| d.as_str().to_string()), false)
        }
    };

    let observe = |data: &[u8]| -> Vec<ChunkObs> {
        chunk_spans(data)
            .into_iter()
            .map(|s| {
                let chunk = &data[s.offset..s.offset + s.length];
                ChunkObs {
                    hash: s.hash,
                    raw_len: s.length as u64,
                    stored_est: cache.estimate(&s.hash, chunk),
                }
            })
            .collect()
    };

    let chunks = observe(payload);
    // Keep the opaque view only when it differs, for the with/without
    // normalization comparison in the report.
    let opaque_chunks = if normalized {
        observe(bytes)
    } else {
        Vec::new()
    };

    let minhash = MinHashSketch::from_hashes(chunks.iter().map(|c| &c.hash));

    ObjectFingerprint {
        bucket: meta.bucket.clone(),
        key: meta.key.clone(),
        logical_size: bytes.len() as u64,
        full_size: meta.size,
        content_type,
        prefix_group,
        excluded: meta.excluded,
        detected_compression: detected,
        normalized,
        chunks,
        opaque_chunks,
        minhash,
        truncated,
    }
}

/// Group key: bucket plus the first `depth` path segments of the key.
pub fn prefix_group(bucket: &str, key: &str, depth: usize) -> String {
    let mut parts = key.split('/');
    let mut prefix = String::new();
    for _ in 0..depth {
        match parts.next() {
            Some(seg) if !seg.is_empty() => {
                prefix.push_str(seg);
                prefix.push('/');
            }
            _ => break,
        }
    }
    if prefix.is_empty() {
        format!("{bucket}/(root)")
    } else {
        format!("{bucket}/{prefix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(key: &str, size: u64) -> ObjectMeta {
        ObjectMeta {
            bucket: "b".into(),
            key: key.into(),
            size,
            storage_class: None,
            excluded: false,
        }
    }

    #[test]
    fn prefix_groups() {
        assert_eq!(prefix_group("b", "snapshots/run1/x.tar", 1), "b/snapshots/");
        assert_eq!(
            prefix_group("b", "snapshots/run1/x.tar", 2),
            "b/snapshots/run1/"
        );
        assert_eq!(prefix_group("b", "toplevel.bin", 1), "b/toplevel.bin/");
    }

    #[test]
    fn identical_objects_share_all_chunks() {
        let cache = StoredSizeCache::default();
        let data: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        let a = fingerprint_bytes(
            &meta("a", data.len() as u64),
            &data,
            None,
            "b/x/".into(),
            true,
            false,
            &cache,
        );
        let b = fingerprint_bytes(
            &meta("b", data.len() as u64),
            &data,
            None,
            "b/x/".into(),
            true,
            false,
            &cache,
        );
        let ha: Vec<_> = a.chunks.iter().map(|c| c.hash).collect();
        let hb: Vec<_> = b.chunks.iter().map(|c| c.hash).collect();
        assert_eq!(ha, hb);
        assert_eq!(a.minhash.jaccard(&b.minhash), 1.0);
    }

    #[test]
    fn gzip_object_normalizes() {
        use std::io::Write;
        let raw: Vec<u8> = (0..2_000_000u32).map(|i| ((i / 90) % 251) as u8).collect();
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        enc.write_all(&raw).unwrap();
        let gz = enc.finish().unwrap();

        let cache = StoredSizeCache::default();
        let fp = fingerprint_bytes(
            &meta("a.gz", gz.len() as u64),
            &gz,
            None,
            "b/x/".into(),
            true,
            false,
            &cache,
        );
        assert!(fp.normalized);
        assert_eq!(fp.detected_compression.as_deref(), Some("gzip"));
        assert!(!fp.opaque_chunks.is_empty());
        // Normalized chunks must equal chunks of the raw payload.
        let raw_hashes: Vec<_> = chunk_spans(&raw).into_iter().map(|s| s.hash).collect();
        let got: Vec<_> = fp.chunks.iter().map(|c| c.hash).collect();
        assert_eq!(got, raw_hashes);
    }

    #[test]
    fn truncated_window_is_opaque() {
        use std::io::Write;
        let raw = vec![7u8; 500_000];
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        enc.write_all(&raw).unwrap();
        let gz = enc.finish().unwrap();
        let window = &gz[..gz.len() / 2];

        let cache = StoredSizeCache::default();
        let fp = fingerprint_bytes(
            &meta("big.gz", gz.len() as u64),
            window,
            None,
            "b/x/".into(),
            true,
            true,
            &cache,
        );
        assert!(!fp.normalized);
        assert!(fp.truncated);
        assert_eq!(fp.logical_size, window.len() as u64);
    }
}
