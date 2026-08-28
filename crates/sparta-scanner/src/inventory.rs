//! S3 Inventory ingestion.
//!
//! Preferred enumeration source for large buckets: no ListObjectsV2 request
//! storm. Supports CSV-format inventories (the S3 default), gzip-compressed as
//! S3 delivers them. The manifest may be a local file path or an
//! `s3://bucket/key` URI; data files referenced by the manifest are fetched
//! relative to the same location.

use std::io::Read;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::s3;
use crate::types::ObjectMeta;

#[derive(Debug, Deserialize)]
struct InventoryManifest {
    #[serde(rename = "sourceBucket")]
    source_bucket: Option<String>,
    #[serde(rename = "fileFormat")]
    file_format: String,
    #[serde(rename = "fileSchema")]
    file_schema: String,
    files: Vec<InventoryFile>,
}

#[derive(Debug, Deserialize)]
struct InventoryFile {
    key: String,
}

/// Where the manifest (and its data files) live.
enum Source<'a> {
    Local(&'a Path),
    S3 { bucket: String },
}

pub async fn enumerate(
    client: &aws_sdk_s3::Client,
    manifest_loc: &str,
    mut sink: impl FnMut(ObjectMeta),
) -> Result<u64> {
    let (manifest_bytes, source) = if let Some(rest) = manifest_loc.strip_prefix("s3://") {
        let (bucket, key) = rest
            .split_once('/')
            .context("inventory manifest URI must be s3://bucket/key")?;
        let got = s3::get_full(client, bucket, key).await?;
        (
            got.bytes,
            Source::S3 {
                bucket: bucket.to_string(),
            },
        )
    } else {
        let p = Path::new(manifest_loc);
        (
            std::fs::read(p).with_context(|| format!("read inventory manifest {manifest_loc}"))?,
            Source::Local(p.parent().unwrap_or(Path::new("."))),
        )
    };

    let manifest: InventoryManifest =
        serde_json::from_slice(&manifest_bytes).context("parse inventory manifest JSON")?;
    if !manifest.file_format.eq_ignore_ascii_case("csv") {
        bail!(
            "inventory fileFormat {:?} unsupported (CSV only in MVP; see DECISIONS.md)",
            manifest.file_format
        );
    }

    // Map schema columns. S3 writes e.g. "Bucket, Key, Size, LastModifiedDate,...".
    let cols: Vec<String> = manifest
        .file_schema
        .split(',')
        .map(|c| c.trim().to_ascii_lowercase())
        .collect();
    let key_col = cols
        .iter()
        .position(|c| c == "key")
        .context("inventory schema missing Key column")?;
    let size_col = cols
        .iter()
        .position(|c| c == "size")
        .context("inventory schema missing Size column")?;
    let bucket_col = cols.iter().position(|c| c == "bucket");
    let storage_col = cols.iter().position(|c| c == "storageclass");
    let fallback_bucket = manifest.source_bucket.clone().unwrap_or_default();

    let mut count = 0u64;
    for f in &manifest.files {
        let raw = match &source {
            Source::Local(dir) => {
                let p = dir.join(&f.key);
                std::fs::read(&p).with_context(|| format!("read inventory data {}", p.display()))?
            }
            Source::S3 { bucket } => s3::get_full(client, bucket, &f.key).await?.bytes,
        };
        // Inventory data files are gzip CSV; accept plain CSV too.
        let text = if raw.starts_with(&[0x1f, 0x8b]) {
            let mut out = Vec::new();
            flate2::read::MultiGzDecoder::new(&raw[..])
                .read_to_end(&mut out)
                .context("gunzip inventory data file")?;
            out
        } else {
            raw
        };
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(&text[..]);
        for rec in rdr.records() {
            let rec = rec.context("parse inventory CSV record")?;
            let Some(key) = rec.get(key_col) else {
                continue;
            };
            let size: u64 = rec.get(size_col).and_then(|s| s.parse().ok()).unwrap_or(0);
            let bucket = bucket_col
                .and_then(|i| rec.get(i))
                .filter(|b| !b.is_empty())
                .unwrap_or(&fallback_bucket);
            count += 1;
            sink(ObjectMeta {
                bucket: bucket.to_string(),
                // Inventory keys are URL-encoded.
                key: urldecode(key),
                size,
                storage_class: storage_col
                    .and_then(|i| rec.get(i))
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                excluded: false,
            });
        }
    }
    Ok(count)
}

/// Minimal percent-decoding for inventory keys (S3 URL-encodes them).
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urldecode_basics() {
        assert_eq!(urldecode("a%2Fb+c"), "a/b c");
        assert_eq!(urldecode("plain-key.tar"), "plain-key.tar");
        assert_eq!(urldecode("bad%zz"), "bad%zz");
    }
}
