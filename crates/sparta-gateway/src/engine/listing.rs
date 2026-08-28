//! Merged ListObjectsV2: upstream raw objects + engine manifests, with
//! `_sparta/` hidden, engine entries winning key collisions, delimiter
//! grouping over the merged stream, and a combined continuation token.
//!
//! Pagination strategy: both sides are re-queried with `start-after`-style
//! cursors (the upstream via `start_after`, the engine via a DB cursor), so a
//! continuation token is just the pair of cursors. Skipping past an emitted
//! CommonPrefix advances a cursor to `prefix + U+10FFFF`, which sorts after
//! every key in that group.

use anyhow::{Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::{keys, Engine};

/// One merged listing entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Object {
        key: String,
        size: u64,
        etag: Option<String>,
        last_modified_ms: Option<u64>,
    },
    CommonPrefix(String),
}

impl Entry {
    fn sort_key(&self) -> &str {
        match self {
            Entry::Object { key, .. } => key,
            Entry::CommonPrefix(p) => p,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Cursor {
    /// start-after for the upstream side.
    u: String,
    /// start-after for the engine side.
    e: String,
}

pub fn encode_token(u: &str, e: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&Cursor {
            u: u.to_string(),
            e: e.to_string(),
        })
        .expect("cursor serializes"),
    )
}

fn decode_token(token: &str) -> Result<Cursor> {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token)
        .context("continuation token base64")?;
    serde_json::from_slice(&raw).context("continuation token json")
}

/// Everything after a group prefix sorts before this suffix.
fn past_prefix(prefix: &str) -> String {
    format!("{prefix}\u{10FFFF}")
}

/// Reduce a key to its listing entry under a delimiter, S3-style.
fn to_entry_key(key: &str, prefix: &str, delimiter: Option<&str>) -> EntryKind {
    if let Some(d) = delimiter {
        if !d.is_empty() {
            let rest = &key[prefix.len()..];
            if let Some(i) = rest.find(d) {
                return EntryKind::Prefix(format!("{prefix}{}{d}", &rest[..i]));
            }
        }
    }
    EntryKind::Key
}

enum EntryKind {
    Key,
    Prefix(String),
}

pub struct MergedPage {
    pub entries: Vec<Entry>,
    pub next_token: Option<String>,
}

impl Engine {
    /// Produce one merged page.
    pub async fn merged_list(
        &self,
        bucket: &str,
        prefix: &str,
        delimiter: Option<&str>,
        max_keys: usize,
        continuation: Option<&str>,
        start_after: Option<&str>,
    ) -> Result<MergedPage> {
        let mut cursor = match continuation {
            Some(t) => decode_token(t)?,
            None => Cursor {
                u: start_after.unwrap_or_default().to_string(),
                e: start_after.unwrap_or_default().to_string(),
            },
        };
        let max_keys = max_keys.clamp(1, 1000);

        let mut entries: Vec<Entry> = Vec::new();
        // Buffered heads of each side.
        let mut up_buf: Vec<Entry> = Vec::new();
        let mut en_buf: Vec<Entry> = Vec::new();
        let mut up_done = false;
        let mut en_done = false;

        // Fill a side's buffer from its cursor.
        // Upstream: list with our prefix and delimiter, start-after cursor.
        // Engine: DB scan grouped by delimiter locally.
        loop {
            if up_buf.is_empty() && !up_done {
                let resp = self
                    .client
                    .list_objects_v2()
                    .bucket(bucket)
                    .prefix(prefix)
                    .set_delimiter(delimiter.map(String::from))
                    .start_after(cursor.u.clone())
                    .max_keys(1000)
                    .send()
                    .await
                    .context("upstream list")?;
                for p in resp.common_prefixes() {
                    if let Some(pfx) = p.prefix() {
                        if !keys::is_engine_key(pfx) {
                            up_buf.push(Entry::CommonPrefix(pfx.to_string()));
                        }
                    }
                }
                for o in resp.contents() {
                    let Some(k) = o.key() else { continue };
                    if keys::is_engine_key(k) {
                        continue;
                    }
                    up_buf.push(Entry::Object {
                        key: k.to_string(),
                        size: o.size().unwrap_or(0).max(0) as u64,
                        etag: o.e_tag().map(String::from),
                        last_modified_ms: o
                            .last_modified()
                            .map(|t| (t.secs().max(0) as u64) * 1000),
                    });
                }
                up_buf.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
                if up_buf.is_empty() {
                    up_done = true;
                }
            }
            if en_buf.is_empty() && !en_done {
                // Pull engine keys and fold delimiter groups locally.
                let rows = self
                    .db
                    .list_engine_objects(bucket, prefix, &cursor.e, 1000)
                    .unwrap_or_default();
                if rows.is_empty() {
                    en_done = true;
                } else {
                    let mut advanced = cursor.e.clone();
                    for row in rows {
                        advanced = row.key.clone();
                        match to_entry_key(&row.key, prefix, delimiter) {
                            EntryKind::Key => en_buf.push(Entry::Object {
                                key: row.key,
                                size: row.logical_size,
                                etag: row.etag,
                                last_modified_ms: Some(row.created_ms),
                            }),
                            EntryKind::Prefix(p) => {
                                if en_buf.last() != Some(&Entry::CommonPrefix(p.clone())) {
                                    en_buf.push(Entry::CommonPrefix(p.clone()));
                                }
                                advanced = past_prefix(&p);
                            }
                        }
                    }
                    cursor.e = advanced.clone();
                    if en_buf.is_empty() {
                        // All rows folded into already-emitted prefixes; loop.
                        continue;
                    }
                }
            }

            if entries.len() >= max_keys {
                break;
            }
            // Merge step: smaller head wins; equal keys -> engine shadows raw.
            let take_engine = match (up_buf.first(), en_buf.first()) {
                (None, None) => break,
                (None, Some(_)) => true,
                (Some(_), None) => false,
                (Some(u), Some(e)) => e.sort_key() <= u.sort_key(),
            };
            if take_engine {
                let e = en_buf.remove(0);
                // Skip a shadowed upstream entry with the identical key.
                if let Some(u) = up_buf.first() {
                    if u.sort_key() == e.sort_key() {
                        let dropped = up_buf.remove(0);
                        cursor.u = match &dropped {
                            Entry::CommonPrefix(p) => past_prefix(p),
                            Entry::Object { key, .. } => key.clone(),
                        };
                    }
                }
                // Deduplicate merged common prefixes.
                if let Entry::CommonPrefix(p) = &e {
                    if entries.last() == Some(&Entry::CommonPrefix(p.clone())) {
                        continue;
                    }
                }
                if let Entry::CommonPrefix(p) = &e {
                    cursor.e = past_prefix(p);
                }
                entries.push(e);
            } else {
                let u = up_buf.remove(0);
                cursor.u = match &u {
                    Entry::CommonPrefix(p) => past_prefix(p),
                    Entry::Object { key, .. } => key.clone(),
                };
                if let Entry::CommonPrefix(p) = &u {
                    if entries.last() == Some(&Entry::CommonPrefix(p.clone())) {
                        continue;
                    }
                    // Engine side may hold the same group; skip it there too.
                    if en_buf.first() == Some(&Entry::CommonPrefix(p.clone())) {
                        en_buf.remove(0);
                        cursor.e = past_prefix(p);
                    }
                }
                entries.push(u);
            }
        }

        let truncated = !(up_done && up_buf.is_empty() && en_done && en_buf.is_empty());
        Ok(MergedPage {
            next_token: truncated.then(|| encode_token(&cursor.u, &cursor.e)),
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_roundtrip() {
        let t = encode_token("a/b", "c/d\u{10FFFF}");
        let c = decode_token(&t).unwrap();
        assert_eq!(c.u, "a/b");
        assert_eq!(c.e, "c/d\u{10FFFF}");
        assert!(decode_token("!!!").is_err());
    }

    #[test]
    fn delimiter_folding() {
        match to_entry_key("snapshots/tpl0/x.tar", "snapshots/", Some("/")) {
            EntryKind::Prefix(p) => assert_eq!(p, "snapshots/tpl0/"),
            _ => panic!("expected prefix"),
        }
        match to_entry_key("snapshots/top.tar", "snapshots/", Some("/")) {
            EntryKind::Key => {}
            _ => panic!("expected key"),
        }
        match to_entry_key("a/b/c", "", None) {
            EntryKind::Key => {}
            _ => panic!("no delimiter means keys only"),
        }
    }

    #[test]
    fn past_prefix_sorts_after_members() {
        let p = "abc/";
        assert!(past_prefix(p).as_str() > "abc/zzzzz");
        assert!(past_prefix(p).as_str() > "abc/\u{7f}");
    }
}
