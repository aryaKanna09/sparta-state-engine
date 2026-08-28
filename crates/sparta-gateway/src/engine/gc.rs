//! Garbage collection: chunk-refcounted mark, compaction, and a two-phase
//! sweep over packs — driven entirely by bucket state so it is correct after
//! any crash or DB loss.
//!
//! Mark: read every manifest, building per-pack referenced-chunk sets.
//!
//! Compact: a pack whose garbage fraction (unreferenced chunk bytes / pack
//! bytes, from its own tail index) reaches `SPARTA_GC_COMPACT_THRESHOLD`
//! (default 0.3) gets rewritten: live chunks move into fresh packs, every
//! manifest referencing the old pack is updated to the new locations, and the
//! old pack becomes unreferenced. Ordering makes every crash window safe: new
//! packs first, then manifests, and the old pack is only removed by a later
//! sweep.
//!
//! Sweep (two-phase): an unreferenced pack gets a candidate marker under
//! `_sparta/gc/candidates/`; once the marker is older than the grace period
//! (`SPARTA_GC_GRACE_DAYS`, default 7) and the pack is *still* unreferenced,
//! it is deleted. Every sweep, dry runs included, is logged to the meter.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sparta_core::manifest::Manifest;
use sparta_meter::record::{Mode, RecordBody};
use tracing::{info, warn};

use super::{keys, now_ms, Engine};

#[derive(Debug, Serialize, Deserialize)]
struct CandidateMarker {
    pack_id: String,
    unreferenced_since_ms: u64,
}

/// What a GC run did (or would do, for dry runs).
#[derive(Debug, Default, Serialize)]
pub struct GcReport {
    pub bucket: String,
    pub dry_run: bool,
    pub manifests_scanned: u64,
    pub packs_total: u64,
    pub packs_referenced: u64,
    /// Packs rewritten because their garbage fraction crossed the threshold
    /// (or that would be, in a dry run).
    pub packs_compacted: Vec<String>,
    /// Unreferenced chunk bytes reclaimed by compaction rewrites.
    pub compacted_garbage_bytes: u64,
    /// Manifests rewritten to point at compacted locations.
    pub manifests_rewritten: u64,
    /// Newly unreferenced packs marked this run (or that would be marked).
    pub packs_marked: Vec<String>,
    /// Packs deleted this run (or that would be deleted).
    pub packs_deleted: Vec<String>,
    pub bytes_freed: u64,
    pub chunks_freed: u64,
    /// Packs re-referenced since their marker was written (marker removed).
    pub markers_cleared: Vec<String>,
}

impl Engine {
    pub async fn gc(&self, bucket: &str, dry_run: bool) -> Result<GcReport> {
        let grace_ms = self.cfg.gc_grace_days * 86_400_000;
        let mut report = GcReport {
            bucket: bucket.to_string(),
            dry_run,
            ..Default::default()
        };

        // --- Mark: per-pack referenced chunk sets from every manifest -------
        let manifest_keys = self
            .list_all(bucket, keys::MANIFEST_PREFIX)
            .await
            .context("list manifests")?;
        // pack_id -> set of referenced chunk hashes
        let mut referenced_chunks: HashMap<String, HashSet<sparta_core::hash::ChunkHash>> =
            HashMap::new();
        // pack_id -> manifests (by bucket key) that reference it
        let mut referencing_manifests: HashMap<String, Vec<String>> = HashMap::new();
        let mut manifests: HashMap<String, Manifest> = HashMap::new();
        for (mkey, _) in &manifest_keys {
            let Some(bytes) = self.get_object_bytes(bucket, mkey).await? else {
                continue;
            };
            let m = match Manifest::from_json(&bytes) {
                Ok(m) => m,
                Err(e) => {
                    warn!(key = mkey, error = %e, "unparseable manifest during GC; skipping");
                    continue;
                }
            };
            report.manifests_scanned += 1;
            for c in &m.chunks {
                referenced_chunks
                    .entry(c.pack_id.clone())
                    .or_default()
                    .insert(c.hash);
                let refs = referencing_manifests.entry(c.pack_id.clone()).or_default();
                if refs.last() != Some(mkey) {
                    refs.push(mkey.clone());
                }
            }
            manifests.insert(mkey.clone(), m);
        }
        let referenced: HashSet<String> = referenced_chunks.keys().cloned().collect();

        // --- Compact: rewrite packs with too much garbage -------------------
        self.compact_packs(
            bucket,
            dry_run,
            &referenced_chunks,
            &referencing_manifests,
            &mut manifests,
            &mut report,
        )
        .await?;
        // Manifests may have moved off some packs; recompute the live set.
        let referenced: HashSet<String> = {
            let mut set = HashSet::new();
            for m in manifests.values() {
                for c in &m.chunks {
                    set.insert(c.pack_id.clone());
                }
            }
            let _ = referenced;
            set
        };

        // --- Inventory packs and existing markers ---------------------------
        let packs = self
            .list_all(bucket, keys::PACK_PREFIX)
            .await
            .context("list packs")?;
        let markers = self
            .list_all(bucket, keys::GC_CANDIDATE_PREFIX)
            .await
            .context("list gc markers")?;
        let marker_ids: HashSet<String> = markers
            .iter()
            .filter_map(|(k, _)| {
                k.strip_prefix(keys::GC_CANDIDATE_PREFIX)?
                    .strip_suffix(".json")
                    .map(String::from)
            })
            .collect();

        report.packs_total = packs.len() as u64;
        let now = now_ms();

        for (pkey, psize) in &packs {
            let Some(pack_id) = keys::pack_id_from_key(pkey) else {
                continue;
            };
            if referenced.contains(pack_id) {
                report.packs_referenced += 1;
                if marker_ids.contains(pack_id) {
                    // Re-referenced: clear the stale marker.
                    report.markers_cleared.push(pack_id.to_string());
                    if !dry_run {
                        let _ = self
                            .delete_object_raw(bucket, &keys::gc_candidate_key(pack_id))
                            .await;
                    }
                }
                continue;
            }
            // Unreferenced.
            if !marker_ids.contains(pack_id) {
                report.packs_marked.push(pack_id.to_string());
                if !dry_run {
                    let marker = CandidateMarker {
                        pack_id: pack_id.to_string(),
                        unreferenced_since_ms: now,
                    };
                    self.put_object_bytes(
                        bucket,
                        &keys::gc_candidate_key(pack_id),
                        serde_json::to_vec(&marker).expect("marker serializes"),
                    )
                    .await
                    .context("write gc marker")?;
                }
                continue;
            }
            // Marked: eligible once the grace period has elapsed.
            let marker_key = keys::gc_candidate_key(pack_id);
            let since = match self.get_object_bytes(bucket, &marker_key).await? {
                Some(bytes) => serde_json::from_slice::<CandidateMarker>(&bytes)
                    .map(|m| m.unreferenced_since_ms)
                    .unwrap_or(now),
                None => now,
            };
            if now.saturating_sub(since) >= grace_ms {
                report.packs_deleted.push(pack_id.to_string());
                report.bytes_freed += psize;
                if !dry_run {
                    self.delete_object_raw(bucket, pkey)
                        .await
                        .context("delete pack")?;
                    let _ = self.delete_object_raw(bucket, &marker_key).await;
                    // Drop the local index rows (chunk count for the report).
                    if let Ok(n) = self.db_forget_pack(pack_id) {
                        report.chunks_freed += n;
                    }
                }
            }
        }

        // Every sweep is logged to the meter, dry runs included.
        let record = RecordBody::GcSweep {
            packs_deleted: report.packs_deleted.len() as u64,
            bytes_freed: report.bytes_freed,
            chunks_freed: report.chunks_freed,
            dry_run,
        };
        if let Err(e) = self.meter.append(now_ms(), Mode::Active, record) {
            warn!(error = %e, "meter append for GC sweep failed");
        }
        let _ = self.meter.sync();

        info!(
            bucket,
            dry_run,
            marked = report.packs_marked.len(),
            deleted = report.packs_deleted.len(),
            bytes_freed = report.bytes_freed,
            "gc sweep complete"
        );
        Ok(report)
    }

    /// Remove a deleted pack's rows from the local index; returns chunk count.
    fn db_forget_pack(&self, pack_id: &str) -> Result<u64> {
        self.db.forget_pack(pack_id)
    }

    /// Rewrite packs whose unreferenced-chunk fraction crosses the threshold.
    /// Live chunks move to new packs; referencing manifests are updated (in
    /// `manifests` too, so the caller's sweep sees the new reference set).
    async fn compact_packs(
        &self,
        bucket: &str,
        dry_run: bool,
        referenced_chunks: &HashMap<String, HashSet<sparta_core::hash::ChunkHash>>,
        referencing_manifests: &HashMap<String, Vec<String>>,
        manifests: &mut HashMap<String, Manifest>,
        report: &mut GcReport,
    ) -> Result<()> {
        use sparta_core::packfile;

        let threshold = self.cfg.gc_compact_threshold;
        if threshold >= 1.0 {
            return Ok(()); // compaction disabled
        }
        for (pack_id, live) in referenced_chunks {
            // Read the pack's own tail index to see everything it holds.
            let pack_key = keys::pack_key(pack_id);
            let Some(pack_bytes) = self.get_object_bytes(bucket, &pack_key).await? else {
                continue;
            };
            let entries = match packfile::read_index(&pack_bytes) {
                Ok(e) => e,
                Err(e) => {
                    warn!(pack_id, error = %e, "unreadable pack index; skipping compaction");
                    continue;
                }
            };
            let total: u64 = entries.iter().map(|e| e.data_len).sum();
            let garbage: u64 = entries
                .iter()
                .filter(|e| !live.contains(&e.hash))
                .map(|e| e.data_len)
                .sum();
            if total == 0 || (garbage as f64 / total as f64) < threshold {
                continue;
            }
            report.packs_compacted.push(pack_id.clone());
            report.compacted_garbage_bytes += garbage;
            if dry_run {
                continue;
            }

            // Rewrite live chunks into a fresh pack (re-using the already
            // encoded bytes: no decompress/recompress).
            let mut builder = packfile::PackBuilder::new();
            for e in entries.iter().filter(|e| live.contains(&e.hash)) {
                let data = &pack_bytes[e.offset as usize..(e.offset + e.data_len) as usize];
                builder.add_encoded(e.hash, e.codec, e.dict_id, e.raw_len, data)?;
            }
            let sealed = builder.seal();
            let new_locations: HashMap<sparta_core::hash::ChunkHash, packfile::PackEntry> =
                sealed.entries.iter().map(|e| (e.hash, *e)).collect();
            self.put_object_bytes(
                bucket,
                &keys::pack_key(&sealed.pack_id),
                sealed.bytes.clone(),
            )
            .await
            .context("compacted pack upload")?;
            let _ = self.db.insert_pack(
                &sealed.pack_id,
                sealed.bytes.len() as u64,
                now_ms(),
                &sealed.entries,
            );

            // Point every referencing manifest at the new locations.
            for mkey in referencing_manifests.get(pack_id).into_iter().flatten() {
                let Some(m) = manifests.get_mut(mkey) else {
                    continue;
                };
                let mut changed = false;
                for c in m.chunks.iter_mut() {
                    if &c.pack_id == pack_id {
                        if let Some(new_e) = new_locations.get(&c.hash) {
                            c.pack_id = sealed.pack_id.clone();
                            c.offset = new_e.offset;
                            c.data_len = new_e.data_len;
                            changed = true;
                        }
                    }
                }
                if changed {
                    self.store_manifest(bucket, m)
                        .await
                        .context("manifest rewrite after compaction")?;
                    report.manifests_rewritten += 1;
                }
            }
            info!(
                bucket,
                old_pack = pack_id,
                new_pack = sealed.pack_id,
                garbage,
                "pack compacted; old pack will age out through the sweep"
            );
        }
        Ok(())
    }
}
