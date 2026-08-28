//! TrailStore v1 (M5): per-corpus dictionary training and age-based tiering.
//!
//! `train-dict` samples payloads under a prefix, trains a zstd dictionary,
//! stores it in the bucket (`_sparta/dicts/`), and records a rule so future
//! writes under that prefix compress against it. `tier` applies configurable
//! age rules (`SPARTA_TIER_RULES` = `prefix=days:CLASS,...`) as storage-class
//! transitions on packs the engine owns — a pack transitions only when every
//! manifest referencing it lives under the rule's prefix.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use sparta_core::hash::ChunkHash;
use sparta_core::manifest::Manifest;
use sparta_meter::record::{Mode, RecordBody};
use tracing::{info, warn};

use super::{keys, now_ms, Engine};

#[derive(Debug, Serialize)]
pub struct TrainReport {
    pub bucket: String,
    pub prefix: String,
    pub dict_id_hex: String,
    pub dict_bytes: usize,
    pub sampled_objects: usize,
    pub sampled_bytes: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct TierReport {
    pub bucket: String,
    pub dry_run: bool,
    pub rules: Vec<String>,
    pub packs_considered: u64,
    /// (pack_id, storage_class) transitions performed (or planned, dry run).
    pub transitions: Vec<(String, String)>,
}

/// One parsed tiering rule.
#[derive(Debug, Clone, PartialEq)]
pub struct TierRule {
    pub prefix: String,
    pub min_age_days: u64,
    pub storage_class: String,
}

/// Parse `prefix=days:CLASS,...`.
pub fn parse_tier_rules(spec: &str) -> Result<Vec<TierRule>> {
    let mut rules = Vec::new();
    for part in spec.split(',').filter(|p| !p.trim().is_empty()) {
        let (prefix, rest) = part
            .split_once('=')
            .with_context(|| format!("tier rule {part:?}: expected prefix=days:CLASS"))?;
        let (days, class) = rest
            .split_once(':')
            .with_context(|| format!("tier rule {part:?}: expected prefix=days:CLASS"))?;
        rules.push(TierRule {
            prefix: prefix.trim().to_string(),
            min_age_days: days.trim().parse().context("tier rule days")?,
            storage_class: class.trim().to_string(),
        });
    }
    Ok(rules)
}

impl Engine {
    /// Train a zstd dictionary from payloads under `prefix` and register it
    /// for future writes there.
    pub async fn train_dict(
        &self,
        bucket: &str,
        prefix: &str,
        max_samples: usize,
        max_sample_bytes: u64,
    ) -> Result<TrainReport> {
        // Sample: engine objects under the prefix, newest-first is fine.
        let rows = self
            .db
            .list_engine_objects(bucket, prefix, "", max_samples)
            .context("list objects for training")?;
        if rows.is_empty() {
            bail!("no engine objects under {bucket}/{prefix} to train from");
        }
        let mut samples: Vec<Vec<u8>> = Vec::new();
        let mut total = 0u64;
        for row in &rows {
            if total >= max_sample_bytes {
                break;
            }
            let Ok(Some(m)) = self.fetch_manifest(bucket, &row.key).await else {
                continue;
            };
            if m.quarantined || m.payload_size == 0 {
                continue;
            }
            let engine = std::sync::Arc::new(self.clone_shallow());
            match super::restore::fetch_payload(&engine, bucket, &row.key, &m).await {
                Ok(p) => {
                    total += p.len() as u64;
                    samples.push(p);
                }
                Err(e) => warn!(key = row.key, error = %e, "sample fetch failed"),
            }
        }
        if samples.len() < 4 {
            bail!(
                "need at least 4 sample objects to train, got {}",
                samples.len()
            );
        }

        let dict = zstd::dict::from_samples(&samples, 110 * 1024)
            .map_err(|e| anyhow::anyhow!("dictionary training failed: {e}"))?;
        let dict_id = ChunkHash::of(&dict).prefix_u64();
        let dict_key = format!("{}{dict_id:016x}.dict", keys::DICT_PREFIX);

        self.put_object_bytes(bucket, &dict_key, dict.clone())
            .await
            .context("dictionary upload")?;
        self.db.insert_dict(dict_id, &dict).context("dict cache")?;
        self.db
            .upsert_dict_rule(bucket, prefix, dict_id)
            .context("dict rule")?;

        // Durable rules map for rebuild-index.
        let mut rules: BTreeMap<String, u64> = self
            .db
            .all_dict_rules(bucket)
            .unwrap_or_default()
            .into_iter()
            .collect();
        rules.insert(prefix.to_string(), dict_id);
        self.put_object_bytes(
            bucket,
            keys::DICT_RULES_KEY,
            serde_json::to_vec_pretty(&rules).expect("rules serialize"),
        )
        .await
        .context("rules upload")?;

        let _ = self.meter.append(
            now_ms(),
            Mode::Active,
            RecordBody::Note {
                text: format!(
                    "train-dict: {bucket}/{prefix} -> {dict_id:016x} ({} bytes from {} samples)",
                    dict.len(),
                    samples.len()
                ),
            },
        );
        info!(
            bucket,
            prefix,
            dict_id = format!("{dict_id:016x}"),
            "dictionary trained"
        );
        Ok(TrainReport {
            bucket: bucket.to_string(),
            prefix: prefix.to_string(),
            dict_id_hex: format!("{dict_id:016x}"),
            dict_bytes: dict.len(),
            sampled_objects: samples.len(),
            sampled_bytes: total,
        })
    }

    /// Apply age-based tiering rules to packs the engine owns.
    pub async fn tier(&self, bucket: &str, dry_run: bool) -> Result<TierReport> {
        let rules = parse_tier_rules(&self.cfg.tier_rules)?;
        let mut report = TierReport {
            bucket: bucket.to_string(),
            dry_run,
            rules: rules
                .iter()
                .map(|r| format!("{}={}d:{}", r.prefix, r.min_age_days, r.storage_class))
                .collect(),
            ..Default::default()
        };
        if rules.is_empty() {
            return Ok(report);
        }

        // Pack -> referencing object keys, from the manifests (bucket truth).
        let manifest_objs = self.list_all(bucket, keys::MANIFEST_PREFIX).await?;
        let mut pack_keys: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut pack_oldest_ms: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        for (mkey, _) in &manifest_objs {
            let Some(bytes) = self.get_object_bytes(bucket, mkey).await? else {
                continue;
            };
            let Ok(m) = Manifest::from_json(&bytes) else {
                continue;
            };
            for c in &m.chunks {
                pack_keys
                    .entry(c.pack_id.clone())
                    .or_default()
                    .push(m.key.clone());
                let e = pack_oldest_ms.entry(c.pack_id.clone()).or_insert(u64::MAX);
                *e = (*e).min(m.created_unix_ms);
            }
        }

        let now = now_ms();
        for (pack_id, obj_keys) in &pack_keys {
            report.packs_considered += 1;
            // A pack tiers under a rule only if EVERY referencing object
            // matches that rule's prefix (conservative: mixed packs stay).
            let Some(rule) = rules
                .iter()
                .find(|r| obj_keys.iter().all(|k| k.starts_with(&r.prefix)))
            else {
                continue;
            };
            let age_ms = now.saturating_sub(*pack_oldest_ms.get(pack_id).unwrap_or(&now));
            if age_ms < rule.min_age_days * 86_400_000 {
                continue;
            }
            report
                .transitions
                .push((pack_id.clone(), rule.storage_class.clone()));
            if dry_run {
                continue;
            }
            let pack_key = keys::pack_key(pack_id);
            let result = self
                .client
                .copy_object()
                .bucket(bucket)
                .key(&pack_key)
                .copy_source(format!("{bucket}/{pack_key}"))
                .storage_class(aws_sdk_s3::types::StorageClass::from(
                    rule.storage_class.as_str(),
                ))
                .metadata_directive(aws_sdk_s3::types::MetadataDirective::Copy)
                .send()
                .await;
            match result {
                Ok(_) => info!(pack_id, class = rule.storage_class, "pack tiered"),
                Err(e) => {
                    warn!(pack_id, error = %e, "storage class transition failed");
                    report.transitions.pop();
                }
            }
        }

        let _ = self.meter.append(
            now_ms(),
            Mode::Active,
            RecordBody::Note {
                text: format!(
                    "tier sweep on {bucket}: {} transitions (dry_run={dry_run})",
                    report.transitions.len()
                ),
            },
        );
        Ok(report)
    }

    /// Cheap clone for internal Arc-needing helpers (all fields are Arcs or
    /// cheap): used by train_dict to reuse the restore path.
    fn clone_shallow(&self) -> Engine {
        Engine {
            db: std::sync::Arc::clone(&self.db),
            client: self.client.clone(),
            meter: std::sync::Arc::clone(&self.meter),
            cfg: std::sync::Arc::clone(&self.cfg),
            cache: None, // helper clone skips the cache
            pin_prefixes: self.pin_prefixes.clone(),
            ingest_notify: tokio::sync::Notify::new(),
            key_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_rules_parse() {
        let rules = parse_tier_rules("snapshots/=30:GLACIER_IR, logs/=7:STANDARD_IA").unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].prefix, "snapshots/");
        assert_eq!(rules[0].min_age_days, 30);
        assert_eq!(rules[0].storage_class, "GLACIER_IR");
        assert_eq!(rules[1].prefix, "logs/");
        assert!(parse_tier_rules("").unwrap().is_empty());
        assert!(parse_tier_rules("bad-rule").is_err());
        assert!(parse_tier_rules("p=notanumber:X").is_err());
    }
}
