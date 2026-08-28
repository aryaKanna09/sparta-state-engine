//! sparta-fixtures: seed a bucket with generated test data.
//!
//! Used by docker-compose.dev.yml, CI, and the scanner scale test. Talks to
//! any S3 endpoint using the testkit's fixed dev credentials (or the standard
//! AWS env chain when `--use-env-creds`).

use anyhow::Result;
use clap::{Parser, Subcommand};
use sparta_testkit::fixtures::{self, FleetSpec};
use sparta_testkit::seed;

#[derive(Parser)]
#[command(name = "sparta-fixtures", version)]
struct Args {
    /// S3 endpoint, e.g. http://127.0.0.1:9000.
    #[arg(long, env = "SPARTA_ENDPOINT")]
    endpoint: String,

    #[arg(long, default_value = "sparta-fixtures")]
    bucket: String,

    #[arg(long, default_value_t = 32)]
    concurrency: usize,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// The simulated agent fleet: snapshots derived from base templates,
    /// raw + gzip variants (the SRF benchmark corpus).
    Fleet {
        #[arg(long, default_value_t = 200)]
        snapshots: usize,
        #[arg(long, default_value_t = 3)]
        templates: usize,
        #[arg(long, default_value_t = 8 * 1024 * 1024)]
        template_bytes: usize,
        #[arg(long, default_value_t = 0.02)]
        mutation_rate: f64,
        #[arg(long, default_value_t = 0x5041_5254)]
        seed: u64,
    },
    /// Many small objects with known redundancy (scanner scale testing).
    Population {
        #[arg(long, default_value_t = 100_000)]
        count: usize,
        #[arg(long, default_value_t = 64 * 1024)]
        size: usize,
        #[arg(long, default_value_t = 40)]
        families: usize,
        #[arg(long, default_value_t = 7)]
        seed: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let Args {
        endpoint,
        bucket,
        concurrency,
        cmd,
    } = Args::parse();
    let client = seed::client_for_endpoint(&endpoint).await;
    seed::create_bucket(&client, &bucket).await?;

    let started = std::time::Instant::now();
    let mut batch = Vec::new();
    let mut total = 0u64;
    let mut bytes = 0u64;

    let flush = |batch: &mut Vec<fixtures::FixtureObject>| -> Result<u64> {
        let objs = std::mem::take(batch);
        let n = objs.len() as u64;
        // Block on each batch so memory stays flat.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(seed::put_objects(
                &client,
                &bucket,
                objs,
                concurrency,
            ))
        })?;
        Ok(n)
    };

    match cmd {
        Cmd::Fleet {
            snapshots,
            templates,
            template_bytes,
            mutation_rate,
            seed: fseed,
        } => {
            let spec = FleetSpec {
                seed: fseed,
                templates,
                snapshots,
                template_bytes,
                mutation_rate,
                ..Default::default()
            };
            fixtures::generate_fleet(&spec, |o| {
                bytes += o.bytes.len() as u64;
                batch.push(o);
                if batch.len() >= 16 {
                    total += flush(&mut batch).expect("seed batch");
                }
            });
        }
        Cmd::Population {
            count,
            size,
            families,
            seed: fseed,
        } => {
            fixtures::generate_small_population(count, size, families, fseed, |o| {
                bytes += o.bytes.len() as u64;
                batch.push(o);
                if batch.len() >= 512 {
                    total += flush(&mut batch).expect("seed batch");
                }
            });
        }
    }
    total += flush(&mut batch)?;

    println!(
        "seeded {} objects ({:.1} MiB) into {} in {:.1}s",
        total,
        bytes as f64 / (1024.0 * 1024.0),
        bucket,
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
