//! sparta-scanner: Shadow A — a standalone, strictly read-only redundancy
//! scanner. Enumerates a bucket population (S3 Inventory preferred,
//! ListObjectsV2 fallback), samples objects with ranged GETs under a byte
//! budget, fingerprints them with the SRF spec pipeline, and emits an SRF-S
//! report as JSON + single-file HTML.

mod estimate;
mod exclude;
mod fingerprint;
mod inventory;
mod report;
mod s3;
mod types;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use clap::Parser;
use futures::stream::{FuturesUnordered, StreamExt};
use tracing::{info, warn};

use estimate::View;
use exclude::{PrefixRule, TagRule};
use fingerprint::StoredSizeCache;
use types::{ObjectFingerprint, ObjectMeta, PopulationStats};

/// Read-only S3 redundancy scanner (Shadow A). Produces an SRF-S estimate.
#[derive(Parser, Debug)]
#[command(name = "sparta-scanner", version)]
struct Args {
    /// Bucket to scan (repeatable).
    #[arg(long = "bucket", required_unless_present = "inventory_manifest")]
    buckets: Vec<String>,

    /// Custom S3 endpoint (MinIO, etc.). Credentials come from the standard
    /// AWS env/profile chain; the scanner only ever reads.
    #[arg(long, env = "SPARTA_ENDPOINT")]
    endpoint: Option<String>,

    #[arg(long, env = "AWS_REGION")]
    region: Option<String>,

    /// S3 Inventory manifest.json — local path or s3://bucket/key. When set,
    /// enumeration uses the inventory instead of ListObjectsV2.
    #[arg(long)]
    inventory_manifest: Option<String>,

    /// Fraction of objects to sample (0,1].
    #[arg(long, default_value_t = 0.01)]
    sample_rate: f64,

    /// Floor on the number of sampled objects.
    #[arg(long, default_value_t = 64)]
    min_samples: usize,

    /// Total download budget, e.g. 50GiB.
    #[arg(long, default_value = "50GiB")]
    byte_budget: String,

    /// Per-object fetch cap; larger objects are fingerprinted on a prefix
    /// window and skip normalization, e.g. 256MiB.
    #[arg(long, default_value = "256MiB")]
    max_object_bytes: String,

    /// Concurrent GETs.
    #[arg(long, default_value_t = 16)]
    concurrency: usize,

    /// RNG seed: same seed + same population => same sample.
    #[arg(long, default_value_t = 42)]
    seed: u64,

    /// Exclusion rule for the addressable SRF: prefix or *-glob matched
    /// against `bucket/key` (repeatable).
    #[arg(long = "exclude-prefix")]
    exclude_prefixes: Vec<String>,

    /// Exclusion rule: object tag key=value (repeatable; checked via
    /// GetObjectTagging on sampled objects only).
    #[arg(long = "exclude-tag")]
    exclude_tags: Vec<String>,

    /// Path segments per prefix group in the breakdown.
    #[arg(long, default_value_t = 1)]
    prefix_depth: usize,

    /// Output directory for report.json and report.html.
    #[arg(long, default_value = "sparta-scan-report")]
    out: String,

    /// Disable compression normalization (detection still runs).
    #[arg(long)]
    no_normalize: bool,

    /// Jaccard threshold for cluster grouping.
    #[arg(long, default_value_t = 0.5)]
    cluster_threshold: f64,
}

fn parse_bytes(s: &str) -> Result<u64> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit() && c != '.');
    let (num, unit) = match split {
        Some(i) => s.split_at(i),
        None => (s, ""),
    };
    let n: f64 = num.parse().with_context(|| format!("bad size {s:?}"))?;
    let mult: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        "g" | "gb" | "gib" => 1 << 30,
        "t" | "tb" | "tib" => 1u64 << 40,
        other => bail!("unknown size unit {other:?} in {s:?}"),
    };
    Ok((n * mult as f64) as u64)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    run(args).await
}

async fn run(args: Args) -> Result<()> {
    if !(args.sample_rate > 0.0 && args.sample_rate <= 1.0) {
        bail!("--sample-rate must be in (0, 1]");
    }
    let byte_budget = parse_bytes(&args.byte_budget)?;
    let max_object_bytes = parse_bytes(&args.max_object_bytes)?.max(1);
    let started = std::time::Instant::now();

    let client = s3::build_client(args.endpoint.as_deref(), args.region.as_deref()).await;
    let prefix_rules: Vec<PrefixRule> = args
        .exclude_prefixes
        .iter()
        .map(|p| PrefixRule::new(p))
        .collect();
    let tag_rules: Vec<TagRule> = args
        .exclude_tags
        .iter()
        .map(|t| TagRule::parse(t))
        .collect::<Result<_>>()?;

    // --- 1. Enumerate the population --------------------------------------
    let mut population: Vec<ObjectMeta> = Vec::new();
    let mut stats = PopulationStats::default();
    {
        let sink = |mut m: ObjectMeta| {
            m.excluded = prefix_rules.iter().any(|r| r.matches(&m.bucket, &m.key));
            stats.object_count += 1;
            stats.total_bytes += m.size;
            if m.excluded {
                stats.excluded_object_count += 1;
                stats.excluded_bytes += m.size;
            }
            population.push(m);
        };
        if let Some(manifest) = &args.inventory_manifest {
            stats.from_inventory = true;
            let n = inventory::enumerate(&client, manifest, sink).await?;
            info!(objects = n, "enumerated population from S3 Inventory");
        } else {
            let mut sink = sink;
            for bucket in &args.buckets {
                let n = s3::list_bucket(&client, bucket, &mut sink).await?;
                info!(bucket, objects = n, "enumerated via ListObjectsV2");
            }
        }
    }
    if population.is_empty() {
        bail!("population is empty: nothing to scan");
    }

    // --- 2. Pick the sample ------------------------------------------------
    let target = ((population.len() as f64 * args.sample_rate).ceil() as usize)
        .max(args.min_samples)
        .min(population.len());
    let indices = estimate::sample_indices(population.len(), target, args.seed);
    info!(
        population = population.len(),
        sampled = indices.len(),
        "sample selected"
    );

    // --- 3. Fetch + fingerprint under the byte budget ----------------------
    let cache = Arc::new(StoredSizeCache::default());
    let budget_used = Arc::new(AtomicU64::new(0));
    let fetch_errors = Arc::new(AtomicU64::new(0));
    let mut budget_exhausted = false;

    let mut tasks = FuturesUnordered::new();
    let mut fingerprints: Vec<ObjectFingerprint> = Vec::with_capacity(indices.len());
    let mut queued = 0usize;

    for &i in &indices {
        let meta = population[i].clone();
        let fetch_len = meta.size.min(max_object_bytes);
        // Reserve budget up front; unbiased because sample order is random.
        let reserved = budget_used.fetch_add(fetch_len, Ordering::SeqCst);
        if reserved + fetch_len > byte_budget {
            budget_used.fetch_sub(fetch_len, Ordering::SeqCst);
            budget_exhausted = true;
            break;
        }
        queued += 1;
        let client = client.clone();
        let cache = Arc::clone(&cache);
        let errors = Arc::clone(&fetch_errors);
        let truncated = meta.size > max_object_bytes;
        let normalize_on = !args.no_normalize;
        let depth = args.prefix_depth;
        tasks.push(tokio::spawn(async move {
            let got = if fetch_len == 0 {
                Ok(s3::GotObject {
                    bytes: Vec::new(),
                    content_type: None,
                })
            } else {
                s3::get_range(&client, &meta.bucket, &meta.key, fetch_len).await
            };
            match got {
                Ok(got) => {
                    let group = fingerprint::prefix_group(&meta.bucket, &meta.key, depth);
                    tokio::task::spawn_blocking(move || {
                        Some(fingerprint::fingerprint_bytes(
                            &meta,
                            &got.bytes,
                            got.content_type,
                            group,
                            normalize_on,
                            truncated,
                            &cache,
                        ))
                    })
                    .await
                    .unwrap_or(None)
                }
                Err(e) => {
                    warn!(bucket = meta.bucket, key = meta.key, error = %e, "fetch failed; skipping object");
                    errors.fetch_add(1, Ordering::Relaxed);
                    None
                }
            }
        }));
        // Keep at most `concurrency` fetches in flight.
        while tasks.len() >= args.concurrency {
            if let Some(Ok(Some(fp))) = tasks.next().await {
                fingerprints.push(fp);
            }
        }
    }
    while let Some(res) = tasks.next().await {
        if let Ok(Some(fp)) = res {
            fingerprints.push(fp);
        }
    }
    info!(
        fingerprinted = fingerprints.len(),
        queued,
        bytes = budget_used.load(Ordering::SeqCst),
        elapsed_s = started.elapsed().as_secs(),
        "sampling complete"
    );

    // --- 4. Tag-based exclusions (sampled objects only) ---------------------
    if !tag_rules.is_empty() {
        for fp in fingerprints.iter_mut() {
            match s3::get_tags(&client, &fp.bucket, &fp.key).await {
                Ok(tags) => {
                    if tag_rules.iter().any(|r| r.matches(&tags)) {
                        fp.excluded = true;
                    }
                }
                Err(e) => {
                    warn!(key = fp.key, error = %e, "GetObjectTagging failed; object stays included")
                }
            }
        }
    }

    // --- 5. Estimates -------------------------------------------------------
    let all: Vec<&ObjectFingerprint> = fingerprints.iter().collect();
    if all.is_empty() {
        bail!("no objects were successfully fingerprinted");
    }
    let srf_s = estimate::estimate_with_ci(&all, View::Normalized);
    let srf_no_norm = estimate::estimate_with_ci(&all, View::Opaque);
    let included: Vec<&ObjectFingerprint> = fingerprints.iter().filter(|f| !f.excluded).collect();
    let addressable_srf = if included.is_empty() {
        srf_s.clone()
    } else {
        estimate::estimate_with_ci(&included, View::Normalized)
    };
    let excluded_sampled_bytes: u64 = fingerprints
        .iter()
        .filter(|f| f.excluded)
        .map(|f| f.logical_size)
        .sum();

    let delta = if srf_no_norm.point > 0.0 {
        srf_s.point / srf_no_norm.point
    } else {
        1.0
    };
    let requires_reverification = !(1.0 / 3.0..=3.0).contains(&delta);

    let truncated_objects = fingerprints.iter().filter(|f| f.truncated).count() as u64;
    let sample_logical: u64 = fingerprints.iter().map(|f| f.logical_size).sum();

    let mut caveats = vec![
        "SRF-S measures redundancy within the sample only; duplicate chunks shared with unsampled objects are not counted, so the fleet-wide SRF is typically at or above this estimate. Treat SRF-S as a conservative lower bound.".to_string(),
        "Stored-size estimates use per-chunk zstd level 3, matching the engine's pack codec but without M5 dictionary/delta gains.".to_string(),
    ];
    if truncated_objects > 0 {
        caveats.push(format!(
            "{truncated_objects} objects larger than the per-object cap were fingerprinted on a prefix window and skipped normalization."
        ));
    }
    if !tag_rules.is_empty() {
        caveats.push(
            "Tag exclusion rules are evaluated on sampled objects only (GetObjectTagging per object)."
                .to_string(),
        );
    }
    if budget_exhausted {
        caveats.push(
            "Byte budget was exhausted before the target sample count; coverage is lower than requested."
                .to_string(),
        );
    }

    let buckets = if args.buckets.is_empty() {
        let mut b: Vec<String> = fingerprints.iter().map(|f| f.bucket.clone()).collect();
        b.sort();
        b.dedup();
        b
    } else {
        args.buckets.clone()
    };

    let report = report::Report {
        tool: "sparta-scanner",
        version: env!("CARGO_PKG_VERSION"),
        generated_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        spec: report::SpecParams {
            normalization_default: !args.no_normalize,
            ..Default::default()
        },
        buckets,
        seed: args.seed,
        sample_rate: args.sample_rate,
        byte_budget,
        bytes_scanned: budget_used.load(Ordering::SeqCst),
        population: stats.clone(),
        sample: report::SampleStats {
            object_count: fingerprints.len() as u64,
            logical_bytes: sample_logical,
            unscanned_tail_bytes: fingerprints
                .iter()
                .map(|f| f.full_size.saturating_sub(f.logical_size))
                .sum(),
            compressed_objects_detected: fingerprints
                .iter()
                .filter(|f| f.detected_compression.is_some())
                .count() as u64,
            normalized_objects: fingerprints.iter().filter(|f| f.normalized).count() as u64,
            truncated_objects,
            fetch_errors: fetch_errors.load(Ordering::Relaxed),
            byte_coverage_fraction: if stats.total_bytes > 0 {
                sample_logical as f64 / stats.total_bytes as f64
            } else {
                0.0
            },
            object_coverage_fraction: fingerprints.len() as f64 / stats.object_count as f64,
            budget_exhausted,
        },
        srf_s: srf_s.clone(),
        srf_s_no_normalization: srf_no_norm,
        normalization_delta_factor: delta,
        requires_reverification,
        addressable: report::AddressableStats {
            excluded_sampled_objects: fingerprints.iter().filter(|f| f.excluded).count() as u64,
            excluded_sampled_bytes,
            srf: addressable_srf,
        },
        by_prefix: estimate::group_rows(&all, View::Normalized, "prefix:"),
        by_content_type: estimate::group_rows(&all, View::Normalized, "ctype:"),
        top_clusters: estimate::clusters(&all, args.cluster_threshold, 10),
        caveats,
    };

    // --- 6. Emit ------------------------------------------------------------
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("create output dir {}", args.out))?;
    let json_path = format!("{}/report.json", args.out);
    let html_path = format!("{}/report.html", args.out);
    std::fs::write(&json_path, report.to_json()?)?;
    std::fs::write(&html_path, report.to_html())?;

    info!(
        srf_s = format!("{:.2}", report.srf_s.point),
        ci = format!("{:.2}-{:.2}", report.srf_s.ci_low, report.srf_s.ci_high),
        addressable = format!("{:.2}", report.addressable.srf.point),
        reverify = requires_reverification,
        elapsed_s = started.elapsed().as_secs(),
        json = json_path,
        html = html_path,
        "scan complete"
    );
    println!(
        "SRF-S {:.2}x (95% CI {:.2}-{:.2}), addressable {:.2}x — report: {}",
        report.srf_s.point,
        report.srf_s.ci_low,
        report.srf_s.ci_high,
        report.addressable.srf.point,
        html_path
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sizes() {
        assert_eq!(parse_bytes("1024").unwrap(), 1024);
        assert_eq!(parse_bytes("64MiB").unwrap(), 64 << 20);
        assert_eq!(parse_bytes("50GiB").unwrap(), 50u64 << 30);
        assert_eq!(parse_bytes("1.5kb").unwrap(), 1536);
        assert!(parse_bytes("10 potatoes").is_err());
    }
}
