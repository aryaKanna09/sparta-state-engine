//! `sparta-gateway rebuild-index`: reconstruct the local metadata DB from the
//! bucket alone. The DB is a cache; losing the volume must cost a rebuild,
//! never data. Manifests embed full pack locations, so reading every manifest
//! restores complete read capability; pack tail indexes remain available as a
//! deeper recovery path for damaged manifests.

use anyhow::{Context, Result};
use sparta_core::manifest::Manifest;
use tracing::{info, warn};

use super::{keys, now_ms, Engine};

#[derive(Debug, Default, serde::Serialize)]
pub struct RebuildReport {
    pub buckets: Vec<String>,
    pub manifests_restored: u64,
    pub packs_indexed: u64,
    pub chunks_indexed: u64,
    pub errors: u64,
}

impl Engine {
    /// Rebuild the index for the given buckets (wipes active tables first).
    pub async fn rebuild_index(&self, buckets: &[String]) -> Result<RebuildReport> {
        let mut report = RebuildReport {
            buckets: buckets.to_vec(),
            ..Default::default()
        };
        self.db.wipe_active_index().context("wipe index tables")?;

        for bucket in buckets {
            let manifest_objs = self
                .list_all(bucket, keys::MANIFEST_PREFIX)
                .await
                .with_context(|| format!("list manifests in {bucket}"))?;
            info!(bucket, manifests = manifest_objs.len(), "rebuilding index");

            for (mkey, _) in &manifest_objs {
                let Some(bytes) = self.get_object_bytes(bucket, mkey).await? else {
                    continue;
                };
                let manifest = match Manifest::from_json(&bytes) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(key = mkey, error = %e, "skipping unparseable manifest");
                        report.errors += 1;
                        continue;
                    }
                };
                let kh = keys::keyhash(&manifest.key);
                if keys::keyhash_from_manifest_key(mkey) != Some(kh.as_str()) {
                    warn!(
                        key = mkey,
                        "manifest name does not match embedded key; skipping"
                    );
                    report.errors += 1;
                    continue;
                }
                self.db
                    .upsert_manifest(
                        bucket,
                        &kh,
                        &manifest.key,
                        manifest.logical_size,
                        manifest.etag.as_deref(),
                        manifest.content_type.as_deref(),
                        manifest.created_unix_ms,
                        &bytes,
                    )
                    .context("manifest restore")?;
                report.manifests_restored += 1;

                // Chunk locations come straight from the manifest refs
                // (parent-delta dictionary refs included).
                for c in &manifest.chunks {
                    self.db
                        .restore_chunk_location(c)
                        .context("chunk location restore")?;
                    report.chunks_indexed += 1;
                    if let Some(d) = &c.dict {
                        self.db
                            .restore_chunk_location(d)
                            .context("dict chunk location restore")?;
                    }
                }
                // LSH bands for parent detection.
                if let Some(sig) = manifest
                    .minhash
                    .clone()
                    .and_then(sparta_core::minhash::MinHashSketch::from_signature)
                {
                    let _ = self.db.upsert_bands(bucket, &kh, &sig.band_keys());
                }
            }

            // Trained dictionaries + rules.
            if let Ok(Some(rules_json)) = self.get_object_bytes(bucket, keys::DICT_RULES_KEY).await
            {
                if let Ok(rules) =
                    serde_json::from_slice::<std::collections::BTreeMap<String, u64>>(&rules_json)
                {
                    for (prefix, dict_id) in rules {
                        let _ = self.db.upsert_dict_rule(bucket, &prefix, dict_id);
                        let _ = self.trained_dict(bucket, dict_id).await;
                    }
                }
            }

            // Pack inventory (sizes for the level gauge and GC reporting).
            let packs = self
                .list_all(bucket, keys::PACK_PREFIX)
                .await
                .with_context(|| format!("list packs in {bucket}"))?;
            for (pkey, size) in &packs {
                if let Some(pack_id) = keys::pack_id_from_key(pkey) {
                    self.db
                        .insert_pack(pack_id, *size, now_ms(), &[])
                        .context("pack restore")?;
                    report.packs_indexed += 1;
                }
            }
        }
        info!(
            manifests = report.manifests_restored,
            chunks = report.chunks_indexed,
            packs = report.packs_indexed,
            errors = report.errors,
            "rebuild-index complete"
        );
        Ok(report)
    }
}
