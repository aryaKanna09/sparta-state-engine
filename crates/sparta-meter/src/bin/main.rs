//! sparta-meter CLI: verify the chain, report savings, draft invoices,
//! export CSV/Parquet.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use sparta_meter::pricing::{ratecard_from_cur, RateCard};
use sparta_meter::{export, log, report};

#[derive(Parser)]
#[command(name = "sparta-meter", version)]
struct Args {
    /// Path to the meter log (JSONL).
    #[arg(long, global = true, default_value = "sparta-meter.jsonl")]
    log: PathBuf,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Revalidate the tamper-evident hash chain end to end.
    Verify,
    /// Net savings over a trailing window (default 30 days) plus VSRR.
    Report {
        #[arg(long, default_value_t = 30)]
        window_days: u64,
        /// AWS CUR export (CSV/.gz) to use the customer's real rates.
        #[arg(long)]
        cur: Option<PathBuf>,
    },
    /// Monthly invoice draft (YYYY-MM, UTC), derived only from the log.
    Invoice {
        #[arg(long)]
        month: String,
        #[arg(long)]
        cur: Option<PathBuf>,
        /// Fee as a fraction of net savings.
        #[arg(long, default_value_t = 0.20)]
        fee_fraction: f64,
    },
    /// Export the full log as CSV or Parquet.
    Export {
        #[arg(long, value_parser = ["csv", "parquet"])]
        format: String,
        #[arg(long)]
        out: PathBuf,
    },
}

fn rates(cur: &Option<PathBuf>) -> Result<RateCard> {
    match cur {
        Some(p) => ratecard_from_cur(p).with_context(|| format!("load CUR {}", p.display())),
        None => Ok(RateCard::aws_us_east_1()),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.cmd {
        Cmd::Verify => {
            let summary = log::verify_file(&args.log)
                .with_context(|| format!("verify {}", args.log.display()))?;
            println!(
                "chain OK: {} records, last hash {}, span {:?}..{:?}",
                summary.record_count, summary.last_hash, summary.first_ts_ms, summary.last_ts_ms
            );
        }
        Cmd::Report { window_days, cur } => {
            let records = log::read_all(&args.log)?;
            let end = records.iter().map(|r| r.ts_ms).max().unwrap_or(0);
            let start = end.saturating_sub(window_days * 86_400_000);
            let card = rates(&cur)?;
            let r = report::savings_report(&records, start, end, &card);
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
        Cmd::Invoice {
            month,
            cur,
            fee_fraction,
        } => {
            let records = log::read_all(&args.log)?;
            let card = rates(&cur)?;
            let inv = report::invoice_draft(&records, &month, &card, fee_fraction)?;
            println!("{}", serde_json::to_string_pretty(&inv)?);
        }
        Cmd::Export { format, out } => {
            let records = log::read_all(&args.log)?;
            match format.as_str() {
                "csv" => export::export_csv(&records, &out)?,
                "parquet" => export::export_parquet(&records, &out)?,
                _ => unreachable!("clap validates"),
            }
            println!("exported {} records to {}", records.len(), out.display());
        }
    }
    Ok(())
}
