//! sparta-gateway: S3-compatible deduplicating storage gateway.
//!
//! M2: full S3 passthrough via s3s + s3s-aws with shadow-mode fingerprinting,
//! Invariant 0 (bypass flag, panic containment, health endpoints), and local
//! shadow statistics. Active-mode dedup lands at M4.

mod active;
mod chunkstream;
mod config;
mod db;
mod engine;
mod http;
mod service;
mod shadow;

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use s3s::auth::SimpleAuth;
use s3s::service::S3ServiceBuilder;
use tracing::{info, warn};

use config::GatewayConfig;
use db::Db;

#[derive(Parser)]
#[command(name = "sparta-gateway", version)]
struct Args {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the gateway (default).
    Serve,
    /// Print shadow-mode statistics from the local DB as JSON.
    Stats,
    /// Rebuild the local metadata DB from the bucket alone.
    RebuildIndex {
        /// Buckets to scan (repeatable).
        #[arg(long = "bucket", required = true)]
        buckets: Vec<String>,
    },
    /// Garbage-collect unreferenced packs (two-phase mark and sweep).
    Gc {
        #[arg(long = "bucket", required = true)]
        buckets: Vec<String>,
        /// Report what would happen without writing or deleting anything.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "info,aws_config=warn,aws_smithy_runtime=warn,hyper_util=warn".into()
            }),
        )
        .init();
    let args = Args::parse();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match args.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => rt.block_on(serve()),
        Cmd::Stats => stats(),
        Cmd::RebuildIndex { buckets } => rt.block_on(async {
            let engine = build_engine().await?;
            let report = engine.rebuild_index(&buckets).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }),
        Cmd::Gc { buckets, dry_run } => rt.block_on(async {
            let engine = build_engine().await?;
            for bucket in &buckets {
                let report = engine.gc(bucket, dry_run).await?;
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            Ok(())
        }),
    }
}

/// Construct an Engine for CLI subcommands (no HTTP server).
async fn build_engine() -> Result<Arc<engine::Engine>> {
    let cfg = Arc::new(GatewayConfig::from_env()?);
    let db = Arc::new(Db::open(Path::new(&cfg.db_path))?);
    let meter = Arc::new(sparta_meter::log::MeterWriter::open(Path::new(
        &cfg.meter_path,
    ))?);
    let client = build_upstream_client(&cfg).await;
    Ok(Arc::new(engine::Engine::new(db, client, meter, cfg)))
}

/// Upstream S3 client shared by serve and the CLI subcommands.
async fn build_upstream_client(cfg: &GatewayConfig) -> aws_sdk_s3::Client {
    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(cfg.upstream_region.clone()))
        .endpoint_url(cfg.upstream_endpoint.clone());
    if let (Some(ak), Some(sk)) = (&cfg.upstream_access_key, &cfg.upstream_secret_key) {
        loader = loader.credentials_provider(aws_sdk_s3::config::Credentials::new(
            ak.clone(),
            sk.clone(),
            None,
            None,
            "sparta-gateway",
        ));
    }
    let shared = loader.load().await;
    let mut conf_builder = aws_sdk_s3::config::Builder::from(&shared)
        .force_path_style(cfg.force_path_style)
        .retry_config(
            aws_sdk_s3::config::retry::RetryConfig::standard()
                .with_max_attempts(cfg.upstream_max_attempts),
        );
    if cfg.upstream_attempt_timeout_ms > 0 {
        conf_builder = conf_builder.timeout_config(
            aws_sdk_s3::config::timeout::TimeoutConfig::builder()
                .operation_attempt_timeout(std::time::Duration::from_millis(
                    cfg.upstream_attempt_timeout_ms,
                ))
                .build(),
        );
    }
    aws_sdk_s3::Client::from_conf(conf_builder.build())
}

fn stats() -> Result<()> {
    let cfg = GatewayConfig::from_env()?;
    let db = Db::open(Path::new(&cfg.db_path))?;
    let snap = db.snapshot()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "ops": snap.ops.iter().cloned().collect::<std::collections::BTreeMap<String, u64>>(),
            "bytes_in": snap.bytes_in,
            "bytes_out": snap.bytes_out,
            "shadow": {
                "logical_bytes": snap.shadow_logical_bytes,
                "unique_raw_bytes": snap.shadow_unique_raw_bytes,
                "unique_stored_bytes": snap.shadow_unique_stored_bytes,
                "chunk_count": snap.shadow_chunk_count,
                "unique_chunk_count": snap.shadow_unique_chunk_count,
                "dedup_srf_lower_bound": snap.shadow_srf(),
            },
        }))?
    );
    Ok(())
}

/// Periodic shadow-mode storage projection into the meter log: logical =
/// bytes written through the engine, physical = the dedup-only unique-stored
/// bound. Labeled `projected`; active mode (M4) emits metered actuals.
fn spawn_level_projection(
    db: Arc<Db>,
    meter: Arc<sparta_meter::log::MeterWriter>,
    interval_s: u64,
) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(interval_s.max(1))).await;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            match db.snapshot() {
                Ok(snap) => {
                    if snap.shadow_logical_bytes == 0 {
                        continue;
                    }
                    let result = meter.append(
                        now_ms,
                        sparta_meter::record::Mode::Shadow,
                        sparta_meter::record::RecordBody::Level {
                            logical_stored_bytes: snap.shadow_logical_bytes,
                            physical_stored_bytes: snap.shadow_unique_stored_bytes,
                            projected: true,
                        },
                    );
                    if let Err(e) = result {
                        warn!(error = %e, "level projection append failed");
                    }
                    let _ = meter.sync();
                }
                Err(e) => warn!(error = %e, "level projection snapshot failed"),
            }
        }
    });
}

async fn serve() -> Result<()> {
    let cfg = Arc::new(GatewayConfig::from_env()?);
    info!(
        upstream = cfg.upstream_endpoint,
        listen = cfg.listen,
        mode = if cfg.active_mode { "active" } else { "shadow" },
        "starting sparta-gateway"
    );

    let db = Arc::new(Db::open(Path::new(&cfg.db_path))?);
    let meter = Arc::new(
        sparta_meter::log::MeterWriter::open(Path::new(&cfg.meter_path))
            .with_context(|| format!("open meter log {}", cfg.meter_path))?,
    );
    if !cfg.active_mode {
        // Shadow: hourly projected level gauge (active emits metered actuals).
        spawn_level_projection(
            Arc::clone(&db),
            Arc::clone(&meter),
            cfg.meter_level_interval_s,
        );
    }

    let client = build_upstream_client(&cfg).await;
    let proxy = s3s_aws::Proxy::from(client.clone());

    let engine = if cfg.active_mode {
        let engine = Arc::new(engine::Engine::new(
            Arc::clone(&db),
            client,
            Arc::clone(&meter),
            Arc::clone(&cfg),
        ));
        // The write-side dedup pipeline (D-022).
        tokio::spawn(engine::ingest::run_worker(Arc::clone(&engine)));
        Some(engine)
    } else {
        None
    };

    let sparta = service::SpartaS3 {
        inner: proxy,
        db: Arc::clone(&db),
        meter,
        cfg: Arc::clone(&cfg),
        engine,
    };

    let mut builder = S3ServiceBuilder::new(sparta);
    match (&cfg.access_key, &cfg.secret_key) {
        (Some(ak), Some(sk)) => {
            builder.set_auth(SimpleAuth::from_single(ak.clone(), sk.clone()));
        }
        _ => {
            warn!("no SPARTA_ACCESS_KEY set: gateway accepts unauthenticated requests (dev mode)");
        }
    }
    let s3_service = builder.build();

    let health = Arc::new(http::Health {
        upstream_ok: AtomicBool::new(false),
    });
    http::spawn_upstream_probe(cfg.upstream_endpoint.clone(), Arc::clone(&health));

    let listener = tokio::net::TcpListener::bind(&cfg.listen)
        .await
        .with_context(|| format!("bind {}", cfg.listen))?;
    http::serve(
        listener,
        http::GatewayHttp {
            s3: s3_service,
            health,
        },
    )
    .await
}
