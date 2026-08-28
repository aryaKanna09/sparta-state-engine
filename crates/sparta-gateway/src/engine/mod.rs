//! The active-mode engine: write-side dedup (via write-through + background
//! ingest), manifest-first reads, merged listings, GC, and index rebuild.
//!
//! Every public entry point is designed to fail *soft*: an `Err` from the
//! engine means "fall back to raw passthrough", never "fail the request".

pub mod cache;
pub mod gc;
pub mod ingest;
pub mod keys;
pub mod listing;
pub mod rebuild;
pub mod restore;
pub mod trailstore;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use tokio::sync::Notify;
use tracing::warn;

use sparta_core::manifest::Manifest;
use sparta_meter::log::MeterWriter;

use crate::config::GatewayConfig;
use crate::db::Db;
use cache::ChunkCache;

type KeyLocks = Mutex<HashMap<(String, String), Arc<tokio::sync::Mutex<()>>>>;

pub struct Engine {
    pub db: Arc<Db>,
    pub client: aws_sdk_s3::Client,
    pub meter: Arc<MeterWriter>,
    pub cfg: Arc<GatewayConfig>,
    pub cache: Option<ChunkCache>,
    pub pin_prefixes: Vec<String>,
    /// Wakes the ingest worker after an enqueue.
    pub ingest_notify: Notify,
    key_locks: KeyLocks,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Engine {
    pub fn new(
        db: Arc<Db>,
        client: aws_sdk_s3::Client,
        meter: Arc<MeterWriter>,
        cfg: Arc<GatewayConfig>,
    ) -> Self {
        let cache = cfg
            .cache_dir
            .as_ref()
            .and_then(|d| ChunkCache::open(d, cfg.cache_bytes, Arc::clone(&db)));
        let pin_prefixes: Vec<String> = std::env::var("SPARTA_PIN_PREFIXES")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        Self {
            db,
            client,
            meter,
            cfg,
            cache,
            pin_prefixes,
            ingest_notify: Notify::new(),
            key_locks: Mutex::new(HashMap::new()),
        }
    }

    /// Per-key async lock: serializes ingest/overwrite/delete on one object.
    /// Single-node by design (multi-node metadata is an explicit non-goal).
    pub async fn lock_key(&self, bucket: &str, key: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let m = {
            let mut map = self.key_locks.lock().unwrap();
            // Opportunistic cleanup: drop unlocked entries when the map grows.
            if map.len() > 4096 {
                map.retain(|_, v| Arc::strong_count(v) > 1);
            }
            Arc::clone(
                map.entry((bucket.to_string(), key.to_string()))
                    .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
            )
        };
        m.lock_owned().await
    }

    /// Manifest lookup: local DB first, bucket object as the durable fallback
    /// (the DB is only an accelerator). Returns the parsed manifest; callers
    /// decide what quarantine means for them.
    pub async fn fetch_manifest(&self, bucket: &str, key: &str) -> Result<Option<Manifest>> {
        let kh = keys::keyhash(key);
        match self.db.get_manifest(bucket, &kh) {
            Ok(Some((json, quarantined))) => {
                let mut m = Manifest::from_json(&json).context("cached manifest parse")?;
                m.quarantined = m.quarantined || quarantined;
                return Ok(Some(m));
            }
            Ok(None) => {}
            Err(e) => warn!(error = %e, "manifest DB lookup failed; trying bucket"),
        }
        // Bucket fallback (also the path that makes a lost DB survivable).
        match self
            .get_object_bytes(bucket, &keys::manifest_key(&kh))
            .await
        {
            Ok(Some(bytes)) => {
                let m = Manifest::from_json(&bytes).context("bucket manifest parse")?;
                let _ = self.db.upsert_manifest(
                    bucket,
                    &kh,
                    &m.key,
                    m.logical_size,
                    m.etag.as_deref(),
                    m.content_type.as_deref(),
                    m.created_unix_ms,
                    &bytes,
                );
                Ok(Some(m))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Persist a manifest durably (bucket first, then the DB cache).
    pub async fn store_manifest(&self, bucket: &str, manifest: &Manifest) -> Result<()> {
        let kh = keys::keyhash(&manifest.key);
        let json = manifest.to_json().context("manifest serialize")?;
        self.put_object_bytes(bucket, &keys::manifest_key(&kh), json.clone())
            .await
            .context("manifest upload")?;
        if let Err(e) = self.db.upsert_manifest(
            bucket,
            &kh,
            &manifest.key,
            manifest.logical_size,
            manifest.etag.as_deref(),
            manifest.content_type.as_deref(),
            manifest.created_unix_ms,
            &json,
        ) {
            warn!(error = %e, "manifest DB cache write failed (bucket copy is durable)");
        }
        Ok(())
    }

    /// Quarantine a manifest everywhere; never fails (best effort).
    pub async fn quarantine(&self, bucket: &str, key: &str) {
        let kh = keys::keyhash(key);
        let _ = self.db.quarantine_manifest(bucket, &kh);
        if let Ok(Some(mut m)) = self.fetch_manifest(bucket, key).await {
            m.quarantined = true;
            if let Ok(json) = m.to_json() {
                let _ = self
                    .put_object_bytes(bucket, &keys::manifest_key(&kh), json)
                    .await;
            }
        }
    }

    /// Trained dictionary bytes by id: DB cache first, bucket fallback.
    pub async fn trained_dict(&self, bucket: &str, dict_id: u64) -> Result<Vec<u8>> {
        if let Ok(Some(bytes)) = self.db.get_dict(dict_id) {
            return Ok(bytes);
        }
        let key = format!("{}{:016x}.dict", keys::DICT_PREFIX, dict_id);
        let Some(bytes) = self.get_object_bytes(bucket, &key).await? else {
            anyhow::bail!("dictionary {dict_id:016x} not found in bucket");
        };
        let _ = self.db.insert_dict(dict_id, &bytes);
        Ok(bytes)
    }

    // --- upstream helpers ---------------------------------------------------

    /// GET a whole object; Ok(None) on 404/NoSuchKey.
    pub async fn get_object_bytes(&self, bucket: &str, key: &str) -> Result<Option<Vec<u8>>> {
        match self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
        {
            Ok(resp) => Ok(Some(
                resp.body
                    .collect()
                    .await
                    .with_context(|| format!("read body {bucket}/{key}"))?
                    .into_bytes()
                    .to_vec(),
            )),
            Err(e) => {
                let missing = e
                    .as_service_error()
                    .map(|se| se.is_no_such_key())
                    .unwrap_or(false)
                    || is_not_found_debug(&e);
                if missing {
                    Ok(None)
                } else {
                    Err(e).with_context(|| format!("GET {bucket}/{key}"))
                }
            }
        }
    }

    /// Ranged GET `[start, end)`.
    pub async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .range(format!("bytes={start}-{}", end.saturating_sub(1)))
            .send()
            .await
            .with_context(|| format!("ranged GET {bucket}/{key} [{start},{end})"))?;
        Ok(resp
            .body
            .collect()
            .await
            .context("read ranged body")?
            .into_bytes()
            .to_vec())
    }

    pub async fn put_object_bytes(&self, bucket: &str, key: &str, body: Vec<u8>) -> Result<()> {
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(ByteStream::from(body))
            .send()
            .await
            .with_context(|| format!("PUT {bucket}/{key}"))?;
        Ok(())
    }

    pub async fn delete_object_raw(&self, bucket: &str, key: &str) -> Result<()> {
        self.client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .with_context(|| format!("DELETE {bucket}/{key}"))?;
        Ok(())
    }

    /// List all keys under a prefix (paged internally), with sizes.
    pub async fn list_all(&self, bucket: &str, prefix: &str) -> Result<Vec<(String, u64)>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(bucket)
                .prefix(prefix)
                .max_keys(1000);
            if let Some(t) = &token {
                req = req.continuation_token(t);
            }
            let resp = req
                .send()
                .await
                .with_context(|| format!("list {bucket}/{prefix}"))?;
            for o in resp.contents() {
                if let Some(k) = o.key() {
                    out.push((k.to_string(), o.size().unwrap_or(0).max(0) as u64));
                }
            }
            if resp.is_truncated() == Some(true) {
                token = resp.next_continuation_token().map(str::to_string);
                if token.is_none() {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(out)
    }
}

/// Last-resort 404 detection for S3-compatible endpoints that omit typed
/// error codes.
pub fn is_not_found_debug(e: &impl std::fmt::Debug) -> bool {
    let s = format!("{e:?}");
    s.contains("NoSuchKey") || s.contains("status: 404") || s.contains("StatusCode(404)")
}
