//! sparta-gateway: S3-compatible deduplicating storage gateway.
//!
//! M2: full S3 passthrough via s3s + s3s-aws with shadow-mode fingerprinting,
//! Invariant 0 (bypass flag, panic containment, health endpoints), and local
//! shadow statistics. Active-mode dedup lands at M4.

mod chunkstream;
mod config;
mod db;
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
    /// Rebuild the local metadata DB from the bucket alone (M4).
    RebuildIndex,
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
        Cmd::RebuildIndex => {
            anyhow::bail!("rebuild-index arrives with active mode (M4)")
        }
    }
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

async fn serve() -> Result<()> {
    let cfg = Arc::new(GatewayConfig::from_env()?);
    if cfg.active_mode {
        anyhow::bail!("SPARTA_MODE=active arrives with M4; run shadow mode");
    }
    info!(
        upstream = cfg.upstream_endpoint,
        listen = cfg.listen,
        mode = "shadow",
        "starting sparta-gateway"
    );

    let db = Arc::new(Db::open(Path::new(&cfg.db_path))?);

    // Upstream client (the customer's real endpoint).
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
    let s3_conf = conf_builder.build();
    let client = aws_sdk_s3::Client::from_conf(s3_conf);
    let proxy = s3s_aws::Proxy::from(client);

    let sparta = service::SpartaS3 {
        inner: proxy,
        db: Arc::clone(&db),
        cfg: Arc::clone(&cfg),
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
