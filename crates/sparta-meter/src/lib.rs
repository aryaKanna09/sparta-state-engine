//! sparta-meter: the append-only, tamper-evident metering log, pricing layer,
//! and savings/invoice reporting.
//!
//! The meter is the product: every savings number Sparta reports must be
//! derivable from this log alone, and the log must prove it wasn't edited.
//! Each record carries a BLAKE3 hash chained over the previous record;
//! `sparta-meter verify` revalidates the chain end to end.

pub mod export;
pub mod log;
pub mod pricing;
pub mod record;
pub mod report;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MeterError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("chain broken at seq {seq}: {reason}")]
    ChainBroken { seq: u64, reason: String },
    #[error("{0}")]
    Invalid(String),
}
