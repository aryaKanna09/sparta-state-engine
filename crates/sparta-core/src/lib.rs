//! Sparta State Engine core library.
//!
//! Chunking, hashing, compression normalization, packfile and manifest formats,
//! and similarity sketching. This crate performs no I/O beyond `std::io` traits;
//! all storage and network concerns live in the gateway/scanner crates.

pub mod checkpoint;
pub mod chunker;
pub mod compression;
pub mod hash;
pub mod manifest;
pub mod minhash;
pub mod packfile;
pub mod srf;

pub use hash::ChunkHash;

/// SRF spec constants. These are published measurement parameters: do not change
/// them without a corresponding edit to `docs/SRF-SPEC.md`.
pub mod spec {
    /// FastCDC minimum chunk size: 256 KiB.
    pub const CDC_MIN_SIZE: u32 = 256 * 1024;
    /// FastCDC average (target) chunk size: 1 MiB.
    pub const CDC_AVG_SIZE: u32 = 1024 * 1024;
    /// FastCDC maximum chunk size: 4 MiB.
    pub const CDC_MAX_SIZE: u32 = 4 * 1024 * 1024;
    /// zstd level used for chunk compression inside packs.
    pub const PACK_ZSTD_LEVEL: i32 = 3;
    /// Packs are sealed once they reach this many bytes of encoded records.
    pub const PACK_TARGET_BYTES: u64 = 48 * 1024 * 1024;
    /// Spec floor for pack size (packs may be smaller only when flushed early).
    pub const PACK_MIN_BYTES: u64 = 32 * 1024 * 1024;
    /// Hard ceiling for pack size.
    pub const PACK_MAX_BYTES: u64 = 64 * 1024 * 1024;
    /// Number of MinHash permutations in a similarity sketch.
    pub const MINHASH_PERMS: usize = 128;
    /// On-disk/on-bucket format version for packs and manifests.
    pub const FORMAT_VERSION: u32 = 1;
}

use thiserror::Error;

/// Errors surfaced by core encoding/decoding paths.
///
/// Callers in the gateway must treat any of these as "fall back to passthrough",
/// never as a reason to fail a customer request (Invariant 0).
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid hex hash: {0}")]
    InvalidHash(String),
    #[error("pack format error: {0}")]
    PackFormat(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("chunk hash mismatch: expected {expected}, got {actual}")]
    ChunkHashMismatch { expected: String, actual: String },
    #[error("compression error: {0}")]
    Compression(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
