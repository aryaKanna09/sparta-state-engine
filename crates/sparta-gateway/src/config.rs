//! Gateway configuration. Everything comes from environment variables; the
//! full table lives in docs/OPERATIONS.md.
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `SPARTA_LISTEN` | `0.0.0.0:8014` | Listen address |
//! | `SPARTA_UPSTREAM_ENDPOINT` | (required) | Customer's real S3 endpoint URL |
//! | `SPARTA_UPSTREAM_REGION` | `us-east-1` | Upstream region |
//! | `SPARTA_ACCESS_KEY` / `SPARTA_SECRET_KEY` | (unset) | Credentials clients sign with; unset = anonymous dev mode |
//! | `SPARTA_UPSTREAM_ACCESS_KEY` / `SPARTA_UPSTREAM_SECRET_KEY` | = gateway keys | Upstream credentials when they differ |
//! | `SPARTA_MODE` | `shadow` | `shadow` or `active` |
//! | `SPARTA_DB_PATH` | `sparta-gateway.sqlite` | Local metadata cache/accelerator |
//! | `SPARTA_BYPASS` | unset | `1` = dumb proxy, checked per request |
//! | `SPARTA_FORCE_PATH_STYLE` | `1` | Path-style addressing upstream |
//! | `SPARTA_SHADOW_CHUNKING` | `1` | Fingerprint streamed bytes in shadow mode |
//! | `SPARTA_GC_GRACE_DAYS` | `7` | GC epoch grace period (M4) |
//! | `SPARTA_CACHE_DIR` | (unset) | NVMe chunk cache directory (M4) |
//! | `SPARTA_CACHE_BYTES` | `10737418240` | Chunk cache capacity (M4) |
//! | `SPARTA_MAX_NORMALIZE_BYTES` | `1073741824` | Largest object normalization will decompress |

use anyhow::{bail, Result};

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub listen: String,
    pub upstream_endpoint: String,
    pub upstream_region: String,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub upstream_access_key: Option<String>,
    pub upstream_secret_key: Option<String>,
    pub active_mode: bool,
    pub db_path: String,
    pub meter_path: String,
    pub meter_level_interval_s: u64,
    /// Max objects pulled per ingest batch.
    pub ingest_batch: usize,
    /// Objects larger than this stay raw passthrough (no in-memory ingest).
    pub ingest_max_bytes: u64,
    /// Parallel coalesced fetches on the restore path.
    pub restore_concurrency: usize,
    /// Max gap between chunks merged into one ranged GET.
    pub coalesce_gap_bytes: u64,
    pub force_path_style: bool,
    pub shadow_chunking: bool,
    /// Per-attempt timeout for upstream calls, ms (0 = SDK default).
    pub upstream_attempt_timeout_ms: u64,
    /// Max attempts for upstream calls (SDK retry layer).
    pub upstream_max_attempts: u32,
    /// Compact a pack once this fraction of its bytes is unreferenced.
    pub gc_compact_threshold: f64,
    pub gc_grace_days: u64,
    #[allow(dead_code)]
    pub cache_dir: Option<String>,
    #[allow(dead_code)]
    pub cache_bytes: u64,
    #[allow(dead_code)]
    pub max_normalize_bytes: u64,
}

fn env_flag(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(v.as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

/// The one flag that is deliberately NOT cached: bypass must turn the gateway
/// into a dumb proxy instantly, per request, no restart.
///
/// Two triggers: `SPARTA_BYPASS=1` in the environment (fixed at process start
/// on most platforms), and — because a running process cannot see external
/// env edits — the existence of the file named by `SPARTA_BYPASS_FILE`, which
/// an operator can `touch`/`rm` (or mount via ConfigMap) with no restart.
pub fn bypass_requested() -> bool {
    if env_flag("SPARTA_BYPASS", false) {
        return true;
    }
    if let Ok(p) = std::env::var("SPARTA_BYPASS_FILE") {
        return std::path::Path::new(&p).exists();
    }
    false
}

impl GatewayConfig {
    pub fn from_env() -> Result<Self> {
        let Ok(upstream_endpoint) = std::env::var("SPARTA_UPSTREAM_ENDPOINT") else {
            bail!("SPARTA_UPSTREAM_ENDPOINT is required (the customer's real S3 endpoint)");
        };
        let access_key = std::env::var("SPARTA_ACCESS_KEY").ok();
        let secret_key = std::env::var("SPARTA_SECRET_KEY").ok();
        if access_key.is_some() != secret_key.is_some() {
            bail!("SPARTA_ACCESS_KEY and SPARTA_SECRET_KEY must be set together");
        }
        let mode = std::env::var("SPARTA_MODE").unwrap_or_else(|_| "shadow".into());
        let active_mode = match mode.as_str() {
            "shadow" => false,
            "active" => true,
            other => bail!("SPARTA_MODE must be 'shadow' or 'active', got {other:?}"),
        };
        Ok(Self {
            listen: std::env::var("SPARTA_LISTEN").unwrap_or_else(|_| "0.0.0.0:8014".into()),
            upstream_endpoint,
            upstream_region: std::env::var("SPARTA_UPSTREAM_REGION")
                .unwrap_or_else(|_| "us-east-1".into()),
            upstream_access_key: std::env::var("SPARTA_UPSTREAM_ACCESS_KEY")
                .ok()
                .or_else(|| access_key.clone()),
            upstream_secret_key: std::env::var("SPARTA_UPSTREAM_SECRET_KEY")
                .ok()
                .or_else(|| secret_key.clone()),
            access_key,
            secret_key,
            active_mode,
            db_path: std::env::var("SPARTA_DB_PATH")
                .unwrap_or_else(|_| "sparta-gateway.sqlite".into()),
            meter_path: std::env::var("SPARTA_METER_PATH")
                .unwrap_or_else(|_| "sparta-meter.jsonl".into()),
            meter_level_interval_s: std::env::var("SPARTA_METER_LEVEL_INTERVAL_S")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
            ingest_batch: std::env::var("SPARTA_INGEST_BATCH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(64),
            ingest_max_bytes: std::env::var("SPARTA_INGEST_MAX_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2 * 1024 * 1024 * 1024),
            restore_concurrency: std::env::var("SPARTA_RESTORE_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8),
            coalesce_gap_bytes: std::env::var("SPARTA_COALESCE_GAP_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1024 * 1024),
            force_path_style: env_flag("SPARTA_FORCE_PATH_STYLE", true),
            shadow_chunking: env_flag("SPARTA_SHADOW_CHUNKING", true),
            upstream_attempt_timeout_ms: std::env::var("SPARTA_UPSTREAM_ATTEMPT_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            upstream_max_attempts: std::env::var("SPARTA_UPSTREAM_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .map(|r| r + 1)
                .unwrap_or(4),
            gc_compact_threshold: std::env::var("SPARTA_GC_COMPACT_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.3),
            gc_grace_days: std::env::var("SPARTA_GC_GRACE_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(7),
            cache_dir: std::env::var("SPARTA_CACHE_DIR").ok(),
            cache_bytes: std::env::var("SPARTA_CACHE_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10 * 1024 * 1024 * 1024),
            max_normalize_bytes: std::env::var("SPARTA_MAX_NORMALIZE_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1024 * 1024 * 1024),
        })
    }
}
