//! Background ingest worker (write-through dedup, DECISIONS.md D-022).
//!
//! Client writes pass through to the upstream raw and land on the ingest
//! queue. This worker drains the queue in order: fetch the raw object,
//! sniff/normalize/chunk/dedup it, append new chunks to a shared pack builder
//! (sealed at spec size across objects — "written in manifest order"), upload
//! sealed packs, then write manifests, and only then delete the raws. A crash
//! at any point leaves the raw in place: no data loss, savings deferred.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{info, warn};

use sparta_core::checkpoint::chunk_checkpoint_aware;
use sparta_core::compression::{normalize, SniffOutcome};
use sparta_core::hash::ChunkHash;
use sparta_core::manifest::{ChunkRef, Manifest};
use sparta_core::minhash::MinHashSketch;
use sparta_core::packfile::{compress_chunk_with_dict, PackBuilder, PackEntry, CODEC_ZSTD_DICT};
use sparta_core::spec;
use sparta_meter::record::{Mode, RecordBody};

use super::{keys, now_ms, Engine};

/// A chunk reference that may point into the still-open pack.
enum PendingRef {
    /// Already durable (dedup hit on an uploaded pack).
    Located(ChunkRef),
    /// Lives in the open builder; pack id assigned at seal. The second field
    /// is the parent-delta dictionary ref, attached at resolution.
    Open(PackEntry, Option<Box<ChunkRef>>),
}

/// One object fully chunked, awaiting its packs and manifest write.
struct PendingObject {
    bucket: String,
    key: String,
    manifest: Manifest,
    refs: Vec<PendingRef>,
    /// Meter accounting gathered during chunking.
    engine_cpu_us: u64,
    raw_get_bytes: u64,
}

/// Worker loop: drain the queue, batch after batch, forever.
pub async fn run_worker(engine: Arc<Engine>) {
    info!("active-mode ingest worker started");
    loop {
        let processed = match process_batch(&engine).await {
            Ok(n) => n,
            Err(e) => {
                warn!(error = %e, "ingest batch failed; raws remain untouched, will retry");
                0
            }
        };
        if processed == 0 {
            // Idle: wait for a nudge or poll.
            tokio::select! {
                _ = engine.ingest_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }
}

/// Process one batch. Returns the number of objects ingested.
pub async fn process_batch(engine: &Arc<Engine>) -> Result<usize> {
    let batch = engine
        .db
        .pending_ingests(engine.cfg.ingest_batch)
        .unwrap_or_default();
    if batch.is_empty() {
        return Ok(0);
    }

    let mut builder = PackBuilder::new();
    let mut pending_objects: Vec<PendingObject> = Vec::new();
    let mut engine_put_requests = 0u64;
    let mut engine_get_requests = 0u64;
    let mut engine_bytes = 0u64;

    for item in &batch {
        // Serialize against concurrent overwrite/delete on this key.
        let _guard = engine.lock_key(&item.bucket, &item.key).await;

        // Metadata for the manifest (content type + upstream ETag), and the
        // size gate: objects beyond the ingest bound stay raw forever (no
        // loss, no savings; disk-spooled ingest is future work, D-025).
        let head = match engine
            .client
            .head_object()
            .bucket(&item.bucket)
            .key(&item.key)
            .send()
            .await
        {
            Ok(h) => Some(h),
            Err(e) => {
                if super::is_not_found_debug(&e) {
                    let _ = engine.db.remove_ingest(&item.bucket, &item.key);
                } else {
                    warn!(bucket = item.bucket, key = item.key, error = %e,
                          "raw HEAD failed; leaving queued");
                }
                continue;
            }
        };
        let size = head
            .as_ref()
            .and_then(|h| h.content_length())
            .unwrap_or(0)
            .max(0) as u64;
        if size > engine.cfg.ingest_max_bytes {
            info!(
                bucket = item.bucket,
                key = item.key,
                size,
                "object exceeds ingest bound; staying raw passthrough"
            );
            let _ = engine.db.remove_ingest(&item.bucket, &item.key);
            continue;
        }

        engine_get_requests += 1;
        let raw = match engine.get_object_bytes(&item.bucket, &item.key).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                // Deleted between HEAD and GET: nothing to ingest.
                let _ = engine.db.remove_ingest(&item.bucket, &item.key);
                continue;
            }
            Err(e) => {
                warn!(bucket = item.bucket, key = item.key, error = %e,
                      "raw fetch failed; leaving queued");
                continue;
            }
        };
        let content_type = head
            .as_ref()
            .and_then(|h| h.content_type().map(String::from));
        let etag = head
            .as_ref()
            .and_then(|h| h.e_tag().map(String::from))
            .or_else(|| item.etag.clone());
        let user_metadata: std::collections::BTreeMap<String, String> = head
            .as_ref()
            .and_then(|h| h.metadata().cloned())
            .unwrap_or_default()
            .into_iter()
            .collect();

        let t0 = Instant::now();
        let object_hash = ChunkHash::of(&raw);
        let outcome = normalize(&raw, true, engine.cfg.max_normalize_bytes);
        let (payload, recipe, detected): (&[u8], _, _) = match &outcome {
            SniffOutcome::Normalized {
                raw: payload,
                recipe,
                detected,
            } => (payload.as_slice(), Some(recipe.clone()), Some(*detected)),
            SniffOutcome::Opaque { detected } => (raw.as_slice(), None, *detected),
        };
        // Checkpoint-aware boundaries (safetensors / PyTorch zip) when
        // detected; generic FastCDC otherwise (M5).
        let spans = chunk_checkpoint_aware(payload);
        let mut sketch = MinHashSketch::new();
        for span in &spans {
            sketch.insert(&span.hash);
        }

        // M5 delta chains: explicit parent header, or MinHash detection.
        let parent = find_parent(engine, &item.bucket, &item.key, &user_metadata, &sketch).await;
        let parent_ctx = match &parent {
            Some(pm) => load_parent_context(engine, &item.bucket, pm).await,
            None => None,
        };
        // Trained corpus dictionary for this prefix, if one exists.
        let trained = match engine.db.dict_rule_for_key(&item.bucket, &item.key) {
            Ok(Some(id)) => engine
                .trained_dict(&item.bucket, id)
                .await
                .ok()
                .map(|bytes| (id, bytes)),
            _ => None,
        };

        let mut refs = Vec::with_capacity(spans.len());
        for span in &spans {
            // Dedup: durable chunks first, then the open builder (add_chunk
            // returns the existing entry for repeats within the builder).
            if let Ok(Some(loc)) = engine.db.lookup_chunk(&span.hash) {
                refs.push(PendingRef::Located(ChunkRef {
                    hash: span.hash,
                    raw_len: span.length as u64,
                    pack_id: loc.pack_id,
                    offset: loc.offset,
                    data_len: loc.data_len,
                    codec: loc.codec,
                    dict_id: loc.dict_id,
                    dict: None,
                }));
                continue;
            }
            let data = &payload[span.offset..span.offset + span.length];
            let (entry, dict_ref) = encode_chunk(
                &mut builder,
                span,
                data,
                parent_ctx.as_ref(),
                trained.as_ref(),
            )
            .context("pack chunk append")?;
            refs.push(PendingRef::Open(entry, dict_ref));
            if builder.is_full() {
                let sealed = std::mem::take(&mut builder).seal();
                engine_put_requests += 1;
                engine_bytes += sealed.bytes.len() as u64;
                seal_upload_resolve(
                    engine,
                    &sealed,
                    &mut pending_objects,
                    &mut refs,
                    Some(&item.bucket),
                )
                .await?;
            }
        }
        let engine_cpu_us = t0.elapsed().as_micros() as u64;

        let manifest = Manifest {
            format_version: spec::FORMAT_VERSION,
            bucket: item.bucket.clone(),
            key: item.key.clone(),
            version_id: None,
            logical_size: raw.len() as u64,
            object_hash,
            content_type,
            user_metadata,
            etag,
            detected_compression: detected.map(|k| k.as_str().to_string()),
            recipe,
            payload_size: payload.len() as u64,
            payload_hash: ChunkHash::of(payload),
            chunks: Vec::new(), // filled after seal
            parent: parent.as_ref().map(|p| p.key.clone()),
            minhash: Some(sketch.signature().to_vec()),
            created_unix_ms: now_ms(),
            quarantined: false,
        };
        let raw_len = raw.len() as u64;
        pending_objects.push(PendingObject {
            bucket: item.bucket.clone(),
            key: item.key.clone(),
            manifest,
            refs,
            engine_cpu_us,
            raw_get_bytes: raw_len,
        });
    }

    // Seal the remainder so every pending ref becomes durable.
    if !builder.is_empty() {
        let sealed = builder.seal();
        engine_put_requests += 1;
        engine_bytes += sealed.bytes.len() as u64;
        let mut none: Vec<PendingRef> = Vec::new();
        seal_upload_resolve(engine, &sealed, &mut pending_objects, &mut none, None).await?;
    }

    // Finalize each object: manifest to bucket+DB, then delete the raw.
    let mut ingested = 0usize;
    for mut obj in pending_objects {
        let _guard = engine.lock_key(&obj.bucket, &obj.key).await;
        // The delete/overwrite race: if the raw is gone now, the object was
        // deleted while we ingested — drop our work instead of resurrecting.
        // (Overwrites re-enqueue, so a newer raw re-ingests and replaces.)
        let chunks: Result<Vec<ChunkRef>> = obj
            .refs
            .drain(..)
            .map(|r| match r {
                PendingRef::Located(c) => Ok(c),
                PendingRef::Open(..) => {
                    anyhow::bail!("unsealed chunk ref survived batch seal (bug)")
                }
            })
            .collect();
        obj.manifest.chunks = chunks?;
        obj.manifest.validate().context("manifest validation")?;

        let raw_still_there = engine
            .client
            .head_object()
            .bucket(&obj.bucket)
            .key(&obj.key)
            .send()
            .await
            .is_ok();
        if !raw_still_there {
            let _ = engine.db.remove_ingest(&obj.bucket, &obj.key);
            continue;
        }

        engine_put_requests += 1; // manifest PUT
        engine.store_manifest(&obj.bucket, &obj.manifest).await?;
        // Register LSH bands so this object can be detected as a parent.
        if let Some(sig) = &obj.manifest.minhash {
            if let Some(sk) = MinHashSketch::from_signature(sig.clone()) {
                let _ =
                    engine
                        .db
                        .upsert_bands(&obj.bucket, &keys::keyhash(&obj.key), &sk.band_keys());
            }
        }
        if let Err(e) = engine.delete_object_raw(&obj.bucket, &obj.key).await {
            warn!(bucket = obj.bucket, key = obj.key, error = %e,
                  "raw delete failed; manifest shadows it, GC of raw deferred");
        }
        let _ = engine.db.remove_ingest(&obj.bucket, &obj.key);

        // Meter: the write operation's true costs, active mode.
        let stored_new: u64 = obj.manifest.referenced_stored_bytes();
        if let Err(e) = engine.meter.append(
            now_ms(),
            Mode::Active,
            RecordBody::Op {
                op: "ingest_object".into(),
                bucket: obj.bucket.clone(),
                logical_bytes: obj.manifest.logical_size,
                physical_bytes: stored_new,
                engine_put_requests: 0, // batch totals recorded below
                engine_get_requests: 0,
                engine_bytes: obj.raw_get_bytes,
                engine_cpu_us: obj.engine_cpu_us,
            },
        ) {
            warn!(error = %e, "meter append failed for ingest");
        }
        ingested += 1;
    }

    // Batch-level engine overhead record (pack/manifest/raw I/O).
    if ingested > 0 {
        let _ = engine.meter.append(
            now_ms(),
            Mode::Active,
            RecordBody::Op {
                op: "ingest_batch_overhead".into(),
                bucket: String::new(),
                logical_bytes: 0,
                physical_bytes: 0,
                engine_put_requests,
                engine_get_requests,
                engine_bytes,
                engine_cpu_us: 0,
            },
        );
        // Refresh the storage level gauge with metered actuals.
        if let Ok((logical, physical)) = engine.db.storage_levels() {
            let _ = engine.meter.append(
                now_ms(),
                Mode::Active,
                RecordBody::Level {
                    logical_stored_bytes: logical,
                    physical_stored_bytes: physical,
                    projected: false,
                },
            );
        }
        let _ = engine.meter.sync();
        info!(objects = ingested, "ingest batch complete");
    }
    Ok(ingested)
}

/// Upload a sealed pack to every bucket that references it (packs live in
/// the same bucket as the manifests that use them; batches mixing buckets
/// duplicate the pack per bucket), then resolve open refs.
async fn seal_upload_resolve(
    engine: &Arc<Engine>,
    sealed: &sparta_core::packfile::SealedPack,
    pending_objects: &mut [PendingObject],
    current_refs: &mut [PendingRef],
    current_bucket: Option<&str>,
) -> Result<()> {
    let mut buckets: std::collections::BTreeSet<String> = pending_objects
        .iter()
        .filter(|o| o.refs.iter().any(|r| matches!(r, PendingRef::Open(..))))
        .map(|o| o.bucket.clone())
        .collect();
    if let Some(b) = current_bucket {
        if current_refs
            .iter()
            .any(|r| matches!(r, PendingRef::Open(..)))
            || buckets.is_empty()
        {
            buckets.insert(b.to_string());
        }
    }
    for b in &buckets {
        upload_pack(engine, b, sealed).await?;
    }
    resolve_open_refs(pending_objects, current_refs, sealed);
    Ok(())
}

async fn upload_pack(
    engine: &Arc<Engine>,
    bucket: &str,
    sealed: &sparta_core::packfile::SealedPack,
) -> Result<()> {
    engine
        .put_object_bytes(
            bucket,
            &keys::pack_key(&sealed.pack_id),
            sealed.bytes.clone(),
        )
        .await
        .context("pack upload")?;
    engine
        .db
        .insert_pack(
            &sealed.pack_id,
            sealed.bytes.len() as u64,
            now_ms(),
            &sealed.entries,
        )
        .context("pack index insert")?;
    Ok(())
}

/// After a seal, rewrite every `Open` ref that landed in this pack.
fn resolve_open_refs(
    pending_objects: &mut [PendingObject],
    current_refs: &mut [PendingRef],
    sealed: &sparta_core::packfile::SealedPack,
) {
    let in_pack: std::collections::HashMap<ChunkHash, &PackEntry> =
        sealed.entries.iter().map(|e| (e.hash, e)).collect();
    let fix = |r: &mut PendingRef| {
        if let PendingRef::Open(entry, dict_ref) = r {
            if let Some(e) = in_pack.get(&entry.hash) {
                *r = PendingRef::Located(ChunkRef {
                    hash: e.hash,
                    raw_len: e.raw_len,
                    pack_id: sealed.pack_id.clone(),
                    offset: e.offset,
                    data_len: e.data_len,
                    codec: e.codec,
                    dict_id: e.dict_id,
                    dict: dict_ref.take(),
                });
            }
        }
    };
    for obj in pending_objects.iter_mut() {
        obj.refs.iter_mut().for_each(fix);
    }
    current_refs.iter_mut().for_each(fix);
}

/// Parent context for delta encoding: the parent's raw payload plus its chunk
/// map (cumulative offsets), so a child chunk at offset X can be encoded
/// against the positionally aligned parent chunk.
struct ParentContext {
    payload: Vec<u8>,
    /// (payload_start, ref) per parent chunk, in order.
    spans: Vec<(u64, ChunkRef)>,
}

impl ParentContext {
    /// The parent chunk covering `offset` (clamped to the last chunk), with
    /// its raw bytes. Only plain (non-dict) parent chunks are usable as
    /// dictionaries, keeping decode chains one level deep.
    fn chunk_covering(&self, offset: u64) -> Option<(&ChunkRef, &[u8])> {
        if self.spans.is_empty() {
            return None;
        }
        let idx = match self.spans.binary_search_by(|(start, _)| start.cmp(&offset)) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        };
        let (start, r) = &self.spans[idx.min(self.spans.len() - 1)];
        if r.codec == CODEC_ZSTD_DICT || r.dict.is_some() {
            return None;
        }
        let lo = *start as usize;
        let hi = (lo + r.raw_len as usize).min(self.payload.len());
        Some((r, &self.payload[lo..hi]))
    }
}

/// Resolve the delta parent: explicit `x-amz-meta-sparta-parent` first, then
/// MinHash LSH detection above the configured Jaccard threshold.
async fn find_parent(
    engine: &Arc<Engine>,
    bucket: &str,
    child_key: &str,
    user_metadata: &std::collections::BTreeMap<String, String>,
    sketch: &MinHashSketch,
) -> Option<Manifest> {
    if !engine.cfg.delta_enabled {
        return None;
    }
    if let Some(pkey) = user_metadata.get("sparta-parent") {
        match engine.fetch_manifest(bucket, pkey).await {
            Ok(Some(m)) if !m.quarantined => return Some(m),
            Ok(_) => {
                warn!(
                    bucket,
                    child_key,
                    parent = pkey,
                    "declared parent unavailable"
                );
            }
            Err(e) => warn!(bucket, child_key, parent = pkey, error = %e, "parent fetch failed"),
        }
    }
    let child_kh = keys::keyhash(child_key);
    let candidates = engine
        .db
        .band_candidates(bucket, &sketch.band_keys(), 8)
        .unwrap_or_default();
    let mut best: Option<(f64, Manifest)> = None;
    for kh in candidates {
        if kh == child_kh {
            continue; // an overwrite's previous version IS a fine parent,
                      // but its chunks are what we dedup against anyway
        }
        let Ok(Some((json, quarantined))) = engine.db.get_manifest(bucket, &kh) else {
            continue;
        };
        if quarantined {
            continue;
        }
        let Ok(m) = Manifest::from_json(&json) else {
            continue;
        };
        let Some(sig) = m.minhash.clone().and_then(MinHashSketch::from_signature) else {
            continue;
        };
        let j = sketch.jaccard(&sig);
        if j >= engine.cfg.delta_threshold && best.as_ref().map(|(bj, _)| j > *bj).unwrap_or(true) {
            best = Some((j, m));
        }
    }
    best.map(|(_, m)| m)
}

/// Materialize the parent payload and chunk map (best effort — delta encoding
/// is an optimization, never a requirement).
async fn load_parent_context(
    engine: &Arc<Engine>,
    bucket: &str,
    parent: &Manifest,
) -> Option<ParentContext> {
    if parent.payload_size > engine.cfg.ingest_max_bytes {
        return None;
    }
    match super::restore::fetch_payload(engine, bucket, &parent.key, parent).await {
        Ok(payload) => {
            let mut spans = Vec::with_capacity(parent.chunks.len());
            let mut pos = 0u64;
            for c in &parent.chunks {
                spans.push((pos, c.clone()));
                pos += c.raw_len;
            }
            Some(ParentContext { payload, spans })
        }
        Err(e) => {
            warn!(bucket, parent = parent.key, error = %e, "parent payload fetch failed; ingesting without delta");
            None
        }
    }
}

/// Choose the best encoding for a new chunk: plain zstd, raw (incompressible),
/// parent-delta, or trained dictionary — whichever is smallest, with a 10%
/// improvement bar for dictionary codecs (a dict ref costs a fetch at read).
fn encode_chunk(
    builder: &mut PackBuilder,
    span: &sparta_core::chunker::ChunkSpan,
    data: &[u8],
    parent_ctx: Option<&ParentContext>,
    trained: Option<&(u64, Vec<u8>)>,
) -> anyhow::Result<(PackEntry, Option<Box<ChunkRef>>)> {
    let level = sparta_core::spec::PACK_ZSTD_LEVEL;
    let plain = zstd::stream::encode_all(data, level)?;
    let plain_len = plain.len().min(data.len());

    // Parent-delta candidate.
    let parent_choice = parent_ctx
        .and_then(|ctx| ctx.chunk_covering(span.offset as u64))
        .and_then(|(p_ref, p_data)| {
            compress_chunk_with_dict(data, p_data, level)
                .ok()
                .filter(|d| d.len() * 10 < plain_len * 9)
                .map(|d| (d, p_ref.clone()))
        });
    if let Some((delta, p_ref)) = parent_choice {
        let entry =
            builder.add_encoded(span.hash, CODEC_ZSTD_DICT, 0, data.len() as u64, &delta)?;
        return Ok((entry, Some(Box::new(p_ref))));
    }

    // Trained-dictionary candidate.
    if let Some((dict_id, dict_bytes)) = trained {
        if let Ok(d) = compress_chunk_with_dict(data, dict_bytes, level) {
            if d.len() * 10 < plain_len * 9 {
                let entry = builder.add_encoded(
                    span.hash,
                    CODEC_ZSTD_DICT,
                    *dict_id,
                    data.len() as u64,
                    &d,
                )?;
                return Ok((entry, None));
            }
        }
    }

    // Plain path (compressed or raw, whichever is smaller).
    let entry = if plain.len() < data.len() {
        builder.add_encoded(
            span.hash,
            sparta_core::packfile::CODEC_ZSTD,
            0,
            data.len() as u64,
            &plain,
        )?
    } else {
        builder.add_encoded(
            span.hash,
            sparta_core::packfile::CODEC_RAW,
            0,
            data.len() as u64,
            data,
        )?
    };
    Ok((entry, None))
}
