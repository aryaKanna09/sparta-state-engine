//! The restore path: reconstruct engine-written objects from packs.
//!
//! Manifest → chunk plan → cache lookups → coalesced parallel ranged GETs →
//! ordered streaming with verification. Full GETs verify the whole object's
//! BLAKE3 against the manifest before the final byte leaves (the last chunk is
//! held back until the hash matches); ranged GETs verify each chunk's own
//! BLAKE3 (DECISIONS.md D-024). On object-hash mismatch: CRITICAL log,
//! manifest quarantined, raw fallback if the caller can provide one.

use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::Bytes;
use futures::stream::{FuturesOrdered, StreamExt};
use tracing::{error, warn};

use sparta_core::hash::{ChunkHash, StreamingHash};
use sparta_core::manifest::{ChunkRef, Manifest};
use sparta_core::packfile::{decode_chunk, PackEntry};

use super::{keys, Engine};

/// A client range request, unresolved (mirrors the HTTP forms).
#[derive(Debug, Clone, Copy)]
pub enum RangeReq {
    /// `bytes=first-last` (last inclusive, open-ended when None).
    Int { first: u64, last: Option<u64> },
    /// `bytes=-length` (final `length` bytes).
    Suffix { length: u64 },
}

/// What the service layer needs to answer a GET.
pub struct RestoredObject {
    /// Streamed payload frames (already trimmed to the requested range).
    pub stream: tokio::sync::mpsc::Receiver<Result<Bytes, std::io::Error>>,
    /// Total bytes that will be streamed.
    pub content_length: u64,
    /// For ranged responses: (start, end_inclusive, full_size).
    pub content_range: Option<(u64, u64, u64)>,
    pub manifest: Manifest,
}

/// A chunk to serve, with the trim window for range requests.
#[derive(Debug, Clone)]
struct PlannedChunk {
    r: ChunkRef,
    /// Slice of the decoded chunk to emit.
    trim_start: u64,
    trim_len: u64,
}

fn entry_of(r: &ChunkRef) -> PackEntry {
    PackEntry {
        hash: r.hash,
        offset: r.offset,
        data_len: r.data_len,
        raw_len: r.raw_len,
        codec: r.codec,
        dict_id: r.dict_id,
    }
}

impl Engine {
    /// Serve an engine object. `range` is the client's requested byte range
    /// over the *object* bytes, half-open `[start, end)` already validated.
    /// Returns Ok(None) if there is no (serviceable) manifest — caller
    /// proxies raw.
    pub async fn restore(
        self: &Arc<Self>,
        bucket: &str,
        key: &str,
        range: Option<RangeReq>,
    ) -> Result<Option<RestoredObject>> {
        let Some(manifest) = self.fetch_manifest(bucket, key).await? else {
            return Ok(None);
        };
        if manifest.quarantined {
            warn!(bucket, key, "manifest quarantined; serving raw fallback");
            return Ok(None);
        }

        // Objects with a recompression recipe cannot be range-served from
        // chunks directly (chunks cover the *raw* payload, the client range
        // addresses the *compressed* object). Reconstruct fully, then slice.
        let needs_full = manifest.recipe.is_some();

        let full_size = manifest.logical_size;
        let (start, end, is_range) = match range {
            None => (0, full_size, false),
            Some(RangeReq::Int { first, last }) => {
                if first >= full_size && full_size > 0 {
                    anyhow::bail!("range not satisfiable: first {first} >= size {full_size}");
                }
                let end = last
                    .map(|l| l.saturating_add(1))
                    .unwrap_or(full_size)
                    .min(full_size);
                (first.min(full_size), end, true)
            }
            Some(RangeReq::Suffix { length }) => {
                (full_size.saturating_sub(length), full_size, true)
            }
        };
        let content_length = end.saturating_sub(start);

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(8);
        let engine = Arc::clone(self);
        let m = manifest.clone();
        let bucket = bucket.to_string();
        let key = key.to_string();
        tokio::spawn(async move {
            let result = if needs_full {
                stream_recompressed(&engine, &bucket, &key, &m, start, end, &tx).await
            } else {
                stream_chunks(&engine, &bucket, &key, &m, start, end, is_range, &tx).await
            };
            if let Err(e) = result {
                error!(bucket, key, error = %e, "restore stream failed");
                let _ = tx
                    .send(Err(std::io::Error::other(format!("restore failed: {e}"))))
                    .await;
            }
        });

        Ok(Some(RestoredObject {
            stream: rx,
            content_length,
            content_range: is_range.then_some((start, end.saturating_sub(1), full_size)),
            manifest,
        }))
    }
}

/// Plan which chunks cover `[start, end)` of the payload and how to trim them.
fn plan(manifest: &Manifest, start: u64, end: u64) -> Vec<PlannedChunk> {
    let mut out = Vec::new();
    let mut pos = 0u64;
    for r in &manifest.chunks {
        let c_start = pos;
        let c_end = pos + r.raw_len;
        pos = c_end;
        if c_end <= start {
            continue;
        }
        if c_start >= end {
            break;
        }
        let trim_start = start.saturating_sub(c_start);
        let trim_end = (end - c_start).min(r.raw_len);
        out.push(PlannedChunk {
            r: r.clone(),
            trim_start,
            trim_len: trim_end - trim_start,
        });
    }
    out
}

/// Fetch one chunk's raw bytes: cache first, then a ranged GET into the pack.
async fn fetch_chunk(
    engine: &Arc<Engine>,
    bucket: &str,
    object_key: &str,
    r: &ChunkRef,
) -> Result<Vec<u8>> {
    if let Some(cache) = &engine.cache {
        if let Some(data) = cache.get(&r.hash) {
            return Ok(data);
        }
    }
    let data = engine
        .get_object_range(
            bucket,
            &keys::pack_key(&r.pack_id),
            r.offset,
            r.offset + r.data_len,
        )
        .await
        .with_context(|| format!("fetch chunk {} from pack {}", r.hash, r.pack_id))?;
    let raw = decode_chunk(&entry_of(r), &data, None).context("chunk decode/verify")?;
    if let Some(cache) = &engine.cache {
        let pinned = super::cache::should_pin(&engine.pin_prefixes, object_key);
        cache.put(&r.hash, &raw, pinned);
    }
    Ok(raw)
}

/// One unit of ordered emission: either a cache hit or a coalesced fetch of
/// adjacent chunks in the same pack (one ranged GET for the whole run).
enum EmitUnit {
    Cached(PlannedChunk, Vec<u8>),
    Fetch(FetchGroup),
}

struct FetchGroup {
    pack_id: String,
    /// Pack byte range [start, end) covering every member.
    start: u64,
    end: u64,
    members: Vec<PlannedChunk>,
}

/// Cap on one coalesced GET; also bounds per-unit memory.
const MAX_GROUP_BYTES: u64 = 32 * 1024 * 1024;

/// Build the emission plan: consult the cache per chunk, coalesce consecutive
/// misses that are adjacent (gap <= coalesce_gap) in the same pack.
fn build_units(engine: &Arc<Engine>, planned: Vec<PlannedChunk>, gap: u64) -> Vec<EmitUnit> {
    let mut units: Vec<EmitUnit> = Vec::new();
    for p in planned {
        let cached = engine.cache.as_ref().and_then(|c| c.get(&p.r.hash));
        if let Some(data) = cached {
            units.push(EmitUnit::Cached(p, data));
            continue;
        }
        if let Some(EmitUnit::Fetch(g)) = units.last_mut() {
            let adjacent = g.pack_id == p.r.pack_id
                && p.r.offset >= g.end
                && p.r.offset - g.end <= gap
                && (p.r.offset + p.r.data_len).saturating_sub(g.start) <= MAX_GROUP_BYTES;
            if adjacent {
                g.end = p.r.offset + p.r.data_len;
                g.members.push(p);
                continue;
            }
        }
        units.push(EmitUnit::Fetch(FetchGroup {
            pack_id: p.r.pack_id.clone(),
            start: p.r.offset,
            end: p.r.offset + p.r.data_len,
            members: vec![p],
        }));
    }
    units
}

/// Resolve one unit into decoded (chunk, raw) pairs, in payload order.
async fn resolve_unit(
    engine: &Arc<Engine>,
    bucket: &str,
    object_key: &str,
    unit: EmitUnit,
) -> Result<Vec<(PlannedChunk, Vec<u8>)>> {
    match unit {
        EmitUnit::Cached(p, data) => Ok(vec![(p, data)]),
        EmitUnit::Fetch(g) => {
            let blob = engine
                .get_object_range(bucket, &keys::pack_key(&g.pack_id), g.start, g.end)
                .await
                .with_context(|| {
                    format!("coalesced fetch pack {} [{},{})", g.pack_id, g.start, g.end)
                })?;
            let mut out = Vec::with_capacity(g.members.len());
            for p in g.members {
                let lo = (p.r.offset - g.start) as usize;
                let hi = lo + p.r.data_len as usize;
                let raw = decode_chunk(&entry_of(&p.r), &blob[lo..hi], None)
                    .context("chunk decode/verify")?;
                if let Some(cache) = &engine.cache {
                    let pinned = super::cache::should_pin(&engine.pin_prefixes, object_key);
                    cache.put(&p.r.hash, &raw, pinned);
                }
                out.push((p, raw));
            }
            Ok(out)
        }
    }
}

/// Ordered streaming with bounded parallel prefetch of coalesced fetch
/// groups. Chunk-hash verification happens in decode; full-object BLAKE3 is
/// checked before the final frame.
#[allow(clippy::too_many_arguments)]
async fn stream_chunks(
    engine: &Arc<Engine>,
    bucket: &str,
    key: &str,
    manifest: &Manifest,
    start: u64,
    end: u64,
    is_range: bool,
    tx: &tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> Result<()> {
    let planned = plan(manifest, start, end);
    // Full-object verification only possible when serving every byte.
    let verify_object = !is_range;
    let mut units: std::collections::VecDeque<EmitUnit> =
        build_units(engine, planned, engine.cfg.coalesce_gap_bytes).into();

    let mut hasher = StreamingHash::new();
    let mut inflight: FuturesOrdered<_> = FuturesOrdered::new();
    let mut emitted: Option<Bytes> = None; // hold-back buffer (final frame)
    let lookahead = engine.cfg.restore_concurrency.max(1);

    loop {
        while inflight.len() < lookahead {
            let Some(unit) = units.pop_front() else { break };
            let engine = Arc::clone(engine);
            let bucket = bucket.to_string();
            let key = key.to_string();
            inflight.push_back(async move { resolve_unit(&engine, &bucket, &key, unit).await });
        }
        let Some(result) = inflight.next().await else {
            break;
        };
        for (p, raw) in result? {
            if verify_object {
                hasher.update(&raw);
            }
            let piece = Bytes::copy_from_slice(
                &raw[p.trim_start as usize..(p.trim_start + p.trim_len) as usize],
            );
            // Hold back one frame so the last is only released post-verify.
            if let Some(prev) = emitted.replace(piece) {
                if tx.send(Ok(prev)).await.is_err() {
                    return Ok(()); // client went away
                }
            }
        }
    }

    if verify_object {
        let actual = hasher.finalize();
        // payload_hash covers the chunked payload; for recipe-less objects it
        // equals object_hash.
        if actual != manifest.payload_hash {
            error!(
                bucket,
                key,
                expected = %manifest.payload_hash,
                actual = %actual,
                "CRITICAL: restored object hash mismatch; quarantining manifest"
            );
            engine.quarantine(bucket, key).await;
            anyhow::bail!("object hash mismatch on restore");
        }
    }
    if let Some(last) = emitted.take() {
        let _ = tx.send(Ok(last)).await;
    }
    Ok(())
}

/// Recipe path: rebuild the raw payload fully, recompress, verify the object
/// hash, then stream (sliced for ranges). Bounded by the normalization size
/// cap at ingest, so full materialization here is acceptable.
async fn stream_recompressed(
    engine: &Arc<Engine>,
    bucket: &str,
    key: &str,
    manifest: &Manifest,
    start: u64,
    end: u64,
    tx: &tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> Result<()> {
    let planned = plan(manifest, 0, manifest.payload_size);
    let mut payload = Vec::with_capacity(manifest.payload_size as usize);
    for p in &planned {
        let raw = fetch_chunk(engine, bucket, key, &p.r).await?;
        payload.extend_from_slice(&raw);
    }
    if ChunkHash::of(&payload) != manifest.payload_hash {
        error!(
            bucket,
            key, "CRITICAL: payload hash mismatch; quarantining manifest"
        );
        engine.quarantine(bucket, key).await;
        anyhow::bail!("payload hash mismatch on restore");
    }
    let recipe = manifest.recipe.as_ref().expect("recipe path");
    let object =
        sparta_core::compression::reproduce(recipe, &payload).context("recipe reproduce")?;
    let actual = ChunkHash::of(&object);
    if actual != manifest.object_hash {
        error!(
            bucket,
            key,
            expected = %manifest.object_hash,
            actual = %actual,
            "CRITICAL: recompressed object hash mismatch; quarantining manifest"
        );
        engine.quarantine(bucket, key).await;
        anyhow::bail!("object hash mismatch after recompression");
    }
    let slice = &object[start as usize..end as usize];
    for frame in slice.chunks(1 << 20) {
        if tx.send(Ok(Bytes::copy_from_slice(frame))).await.is_err() {
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn manifest_with_chunks(lens: &[u64]) -> Manifest {
        let chunks: Vec<ChunkRef> = lens
            .iter()
            .enumerate()
            .map(|(i, len)| ChunkRef {
                hash: ChunkHash::of(&(i as u64).to_le_bytes()),
                raw_len: *len,
                pack_id: "ab".repeat(32),
                offset: 0,
                data_len: *len,
                codec: 0,
                dict_id: 0,
            })
            .collect();
        let total: u64 = lens.iter().sum();
        Manifest {
            format_version: sparta_core::spec::FORMAT_VERSION,
            bucket: "b".into(),
            key: "k".into(),
            version_id: None,
            logical_size: total,
            object_hash: ChunkHash::of(b""),
            content_type: None,
            user_metadata: BTreeMap::new(),
            etag: None,
            detected_compression: None,
            recipe: None,
            payload_size: total,
            payload_hash: ChunkHash::of(b""),
            chunks,
            parent: None,
            minhash: None,
            created_unix_ms: 0,
            quarantined: false,
        }
    }

    #[test]
    fn plan_full_object() {
        let m = manifest_with_chunks(&[100, 200, 300]);
        let p = plan(&m, 0, 600);
        assert_eq!(p.len(), 3);
        assert_eq!((p[0].trim_start, p[0].trim_len), (0, 100));
        assert_eq!((p[2].trim_start, p[2].trim_len), (0, 300));
    }

    #[test]
    fn plan_interior_range() {
        let m = manifest_with_chunks(&[100, 200, 300]);
        // [150, 350): tail of chunk 1 (50..200) and head of chunk 2 (0..50).
        let p = plan(&m, 150, 350);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].trim_start, p[0].trim_len), (50, 150));
        assert_eq!((p[1].trim_start, p[1].trim_len), (0, 50));
    }

    #[test]
    fn plan_single_chunk_slice() {
        let m = manifest_with_chunks(&[100, 200, 300]);
        let p = plan(&m, 120, 180);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].trim_start, p[0].trim_len), (20, 60));
    }

    #[test]
    fn plan_clamps_at_end() {
        let m = manifest_with_chunks(&[100]);
        let p = plan(&m, 50, 100);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].trim_start, p[0].trim_len), (50, 50));
    }
}
