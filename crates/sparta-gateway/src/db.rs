//! Local metadata database (SQLite, WAL mode).
//!
//! The DB is a cache and accelerator only. Every fact needed to reconstruct an
//! object also lives in the bucket under `_sparta/` (manifests, pack tail
//! indexes); losing this file costs a rebuild, never data.
//!
//! At M2 it holds shadow-mode statistics: request counters, byte counters, and
//! the observed-chunk set behind the shadow dedup estimate.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::Connection;

use sparta_core::hash::ChunkHash;

pub struct Db {
    conn: Mutex<Connection>,
}

/// A snapshot of shadow-mode counters for reporting.
#[derive(Debug, Default, serde::Serialize)]
pub struct ShadowSnapshot {
    pub ops: Vec<(String, u64)>,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub shadow_logical_bytes: u64,
    pub shadow_unique_raw_bytes: u64,
    pub shadow_unique_stored_bytes: u64,
    pub shadow_chunk_count: u64,
    pub shadow_unique_chunk_count: u64,
}

impl ShadowSnapshot {
    /// The savings ratio the engine would have achieved on writes it saw.
    pub fn shadow_srf(&self) -> f64 {
        if self.shadow_unique_stored_bytes == 0 {
            return 1.0;
        }
        self.shadow_logical_bytes as f64 / self.shadow_unique_stored_bytes as f64
    }
}

/// Fault-suite hook: simulate a dead/corrupt metadata DB mid-request. All DB
/// entry points fail while the env var is set; requests must still succeed.
fn db_fault() -> Result<()> {
    if std::env::var("SPARTA_FAULT_DB_FAIL").is_ok_and(|v| v == "1") {
        anyhow::bail!("injected DB failure (SPARTA_FAULT_DB_FAIL)");
    }
    Ok(())
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn =
            Connection::open(path).with_context(|| format!("open sqlite db {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS counters (
                name  TEXT PRIMARY KEY,
                value INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS shadow_chunks (
                hash       BLOB PRIMARY KEY,
                raw_len    INTEGER NOT NULL,
                stored_est INTEGER NOT NULL
            );
            -- Active mode: chunk location index (accelerator; the source of
            -- truth is the pack tail indexes + manifests in the bucket).
            CREATE TABLE IF NOT EXISTS chunks (
                hash     BLOB PRIMARY KEY,
                pack_id  TEXT NOT NULL,
                offset   INTEGER NOT NULL,
                data_len INTEGER NOT NULL,
                raw_len  INTEGER NOT NULL,
                codec    INTEGER NOT NULL,
                dict_id  INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS packs (
                pack_id    TEXT PRIMARY KEY,
                size       INTEGER NOT NULL,
                created_ms INTEGER NOT NULL
            );
            -- Manifest cache: JSON mirrors _sparta/manifests/<keyhash>.json.
            CREATE TABLE IF NOT EXISTS manifests (
                bucket        TEXT NOT NULL,
                keyhash       TEXT NOT NULL,
                key           TEXT NOT NULL,
                logical_size  INTEGER NOT NULL,
                etag          TEXT,
                content_type  TEXT,
                created_ms    INTEGER NOT NULL,
                quarantined   INTEGER NOT NULL DEFAULT 0,
                manifest_json BLOB NOT NULL,
                PRIMARY KEY (bucket, keyhash)
            );
            CREATE INDEX IF NOT EXISTS manifests_by_key
                ON manifests (bucket, key);
            -- Write-through ingest queue (drained by the background worker).
            CREATE TABLE IF NOT EXISTS ingest_queue (
                bucket      TEXT NOT NULL,
                key         TEXT NOT NULL,
                etag        TEXT,
                enqueued_ms INTEGER NOT NULL,
                PRIMARY KEY (bucket, key)
            );
            -- MinHash LSH bands for parent detection (M5 delta chains).
            CREATE TABLE IF NOT EXISTS manifest_bands (
                band    INTEGER NOT NULL,
                bucket  TEXT NOT NULL,
                keyhash TEXT NOT NULL,
                PRIMARY KEY (band, bucket, keyhash)
            );
            -- Trained zstd dictionaries (M5 TrailStore) + per-prefix rules.
            CREATE TABLE IF NOT EXISTS dicts (
                dict_id INTEGER PRIMARY KEY,
                bytes   BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS dict_rules (
                bucket  TEXT NOT NULL,
                prefix  TEXT NOT NULL,
                dict_id INTEGER NOT NULL,
                PRIMARY KEY (bucket, prefix)
            );
            -- NVMe chunk cache accounting.
            CREATE TABLE IF NOT EXISTS cache_entries (
                hash         BLOB PRIMARY KEY,
                size         INTEGER NOT NULL,
                last_used_ms INTEGER NOT NULL,
                pinned       INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn bump(&self, name: &str, delta: u64) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO counters(name, value) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET value = value + ?2",
            rusqlite::params![name, delta as i64],
        )?;
        Ok(())
    }

    /// Record one completed shadow-tapped stream: op-level byte count plus the
    /// chunk observations, all in one transaction.
    pub fn record_shadow_stream(
        &self,
        logical_bytes: u64,
        chunks: &[(ChunkHash, u64, u64)], // (hash, raw_len, stored_est)
    ) -> Result<()> {
        db_fault()?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut bump = tx.prepare_cached(
                "INSERT INTO counters(name, value) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET value = value + ?2",
            )?;
            bump.execute(rusqlite::params![
                "shadow_logical_bytes",
                logical_bytes as i64
            ])?;
            bump.execute(rusqlite::params!["shadow_chunk_count", chunks.len() as i64])?;

            let mut insert = tx.prepare_cached(
                "INSERT OR IGNORE INTO shadow_chunks(hash, raw_len, stored_est)
                 VALUES (?1, ?2, ?3)",
            )?;
            let mut new_raw = 0i64;
            let mut new_stored = 0i64;
            let mut new_count = 0i64;
            for (hash, raw_len, stored_est) in chunks {
                if insert.execute(rusqlite::params![
                    hash.as_bytes().as_slice(),
                    *raw_len as i64,
                    *stored_est as i64
                ])? > 0
                {
                    new_raw += *raw_len as i64;
                    new_stored += *stored_est as i64;
                    new_count += 1;
                }
            }
            bump.execute(rusqlite::params!["shadow_unique_raw_bytes", new_raw])?;
            bump.execute(rusqlite::params!["shadow_unique_stored_bytes", new_stored])?;
            bump.execute(rusqlite::params!["shadow_unique_chunk_count", new_count])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn counter(&self, name: &str) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let v: Option<i64> = conn
            .query_row("SELECT value FROM counters WHERE name = ?1", [name], |r| {
                r.get(0)
            })
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })?;
        Ok(v.unwrap_or(0) as u64)
    }

    pub fn snapshot(&self) -> Result<ShadowSnapshot> {
        let ops = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT name, value FROM counters WHERE name LIKE 'op_%' ORDER BY name")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        Ok(ShadowSnapshot {
            ops,
            bytes_in: self.counter("bytes_in")?,
            bytes_out: self.counter("bytes_out")?,
            shadow_logical_bytes: self.counter("shadow_logical_bytes")?,
            shadow_unique_raw_bytes: self.counter("shadow_unique_raw_bytes")?,
            shadow_unique_stored_bytes: self.counter("shadow_unique_stored_bytes")?,
            shadow_chunk_count: self.counter("shadow_chunk_count")?,
            shadow_unique_chunk_count: self.counter("shadow_unique_chunk_count")?,
        })
    }
}

/// Location of one chunk inside a pack (mirrors `sparta_core::packfile::PackEntry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkLocation {
    pub pack_id: String,
    pub offset: u64,
    pub data_len: u64,
    pub raw_len: u64,
    pub codec: u8,
    pub dict_id: u64,
}

/// A row in the engine listing (merged into ListObjectsV2 output).
#[derive(Debug, Clone)]
pub struct EngineObject {
    pub key: String,
    pub logical_size: u64,
    pub etag: Option<String>,
    pub created_ms: u64,
}

/// One pending write-through ingest.
#[derive(Debug, Clone)]
pub struct PendingIngest {
    pub bucket: String,
    pub key: String,
    pub etag: Option<String>,
}

impl Db {
    pub fn lookup_chunk(&self, hash: &ChunkHash) -> Result<Option<ChunkLocation>> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT pack_id, offset, data_len, raw_len, codec, dict_id
                 FROM chunks WHERE hash = ?1",
                [hash.as_bytes().as_slice()],
                |r| {
                    Ok(ChunkLocation {
                        pack_id: r.get(0)?,
                        offset: r.get::<_, i64>(1)? as u64,
                        data_len: r.get::<_, i64>(2)? as u64,
                        raw_len: r.get::<_, i64>(3)? as u64,
                        codec: r.get::<_, i64>(4)? as u8,
                        dict_id: r.get::<_, i64>(5)? as u64,
                    })
                },
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })?;
        Ok(row)
    }

    /// Register a sealed pack and all its chunk locations in one transaction.
    pub fn insert_pack(
        &self,
        pack_id: &str,
        size: u64,
        created_ms: u64,
        entries: &[sparta_core::packfile::PackEntry],
    ) -> Result<()> {
        db_fault()?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO packs (pack_id, size, created_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params![pack_id, size as i64, created_ms as i64],
        )?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO chunks
                 (hash, pack_id, offset, data_len, raw_len, codec, dict_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for e in entries {
                stmt.execute(rusqlite::params![
                    e.hash.as_bytes().as_slice(),
                    pack_id,
                    e.offset as i64,
                    e.data_len as i64,
                    e.raw_len as i64,
                    e.codec as i64,
                    e.dict_id as i64,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_manifest(
        &self,
        bucket: &str,
        keyhash: &str,
        key: &str,
        logical_size: u64,
        etag: Option<&str>,
        content_type: Option<&str>,
        created_ms: u64,
        manifest_json: &[u8],
    ) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO manifests
             (bucket, keyhash, key, logical_size, etag, content_type, created_ms, quarantined, manifest_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)",
            rusqlite::params![
                bucket,
                keyhash,
                key,
                logical_size as i64,
                etag,
                content_type,
                created_ms as i64,
                manifest_json
            ],
        )?;
        Ok(())
    }

    /// Returns (manifest_json, quarantined).
    pub fn get_manifest(&self, bucket: &str, keyhash: &str) -> Result<Option<(Vec<u8>, bool)>> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT manifest_json, quarantined FROM manifests WHERE bucket = ?1 AND keyhash = ?2",
            rusqlite::params![bucket, keyhash],
            |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)? != 0)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e.into()),
        })
    }

    pub fn delete_manifest(&self, bucket: &str, keyhash: &str) -> Result<bool> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM manifests WHERE bucket = ?1 AND keyhash = ?2",
            rusqlite::params![bucket, keyhash],
        )?;
        Ok(n > 0)
    }

    pub fn quarantine_manifest(&self, bucket: &str, keyhash: &str) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE manifests SET quarantined = 1 WHERE bucket = ?1 AND keyhash = ?2",
            rusqlite::params![bucket, keyhash],
        )?;
        Ok(())
    }

    /// Engine keys for merged listings: keys > start_after matching prefix,
    /// ascending, at most `limit`.
    pub fn list_engine_objects(
        &self,
        bucket: &str,
        prefix: &str,
        start_after: &str,
        limit: usize,
    ) -> Result<Vec<EngineObject>> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT key, logical_size, etag, created_ms FROM manifests
             WHERE bucket = ?1 AND quarantined = 0 AND key > ?2 AND key LIKE ?3 ESCAPE '\\'
             ORDER BY key ASC LIMIT ?4",
        )?;
        let like = format!(
            "{}%",
            prefix
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let rows = stmt
            .query_map(
                rusqlite::params![bucket, start_after, like, limit as i64],
                |r| {
                    Ok(EngineObject {
                        key: r.get(0)?,
                        logical_size: r.get::<_, i64>(1)? as u64,
                        etag: r.get(2)?,
                        created_ms: r.get::<_, i64>(3)? as u64,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn enqueue_ingest(
        &self,
        bucket: &str,
        key: &str,
        etag: Option<&str>,
        ms: u64,
    ) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO ingest_queue (bucket, key, etag, enqueued_ms)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![bucket, key, etag, ms as i64],
        )?;
        Ok(())
    }

    pub fn pending_ingests(&self, limit: usize) -> Result<Vec<PendingIngest>> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT bucket, key, etag FROM ingest_queue ORDER BY enqueued_ms ASC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit as i64], |r| {
                Ok(PendingIngest {
                    bucket: r.get(0)?,
                    key: r.get(1)?,
                    etag: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn remove_ingest(&self, bucket: &str, key: &str) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM ingest_queue WHERE bucket = ?1 AND key = ?2",
            rusqlite::params![bucket, key],
        )?;
        Ok(())
    }

    pub fn ingest_queue_len(&self) -> Result<u64> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM ingest_queue", [], |r| r.get(0))?;
        Ok(n as u64)
    }

    /// (logical stored, physical stored) for the active-mode level gauge.
    pub fn storage_levels(&self) -> Result<(u64, u64)> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let logical: i64 = conn.query_row(
            "SELECT COALESCE(SUM(logical_size), 0) FROM manifests WHERE quarantined = 0",
            [],
            |r| r.get(0),
        )?;
        let physical: i64 =
            conn.query_row("SELECT COALESCE(SUM(size), 0) FROM packs", [], |r| r.get(0))?;
        Ok((logical as u64, physical as u64))
    }

    /// Drop a deleted pack's index rows; returns the chunk count removed.
    pub fn forget_pack(&self, pack_id: &str) -> Result<u64> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM chunks WHERE pack_id = ?1", [pack_id])?;
        conn.execute("DELETE FROM packs WHERE pack_id = ?1", [pack_id])?;
        Ok(n as u64)
    }

    /// Restore one chunk location from a manifest ref (rebuild-index).
    pub fn restore_chunk_location(&self, c: &sparta_core::manifest::ChunkRef) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO chunks
             (hash, pack_id, offset, data_len, raw_len, codec, dict_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                c.hash.as_bytes().as_slice(),
                c.pack_id,
                c.offset as i64,
                c.data_len as i64,
                c.raw_len as i64,
                c.codec as i64,
                c.dict_id as i64,
            ],
        )?;
        Ok(())
    }

    /// Wipe all active-mode index tables (used by rebuild-index).
    pub fn wipe_active_index(&self) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("DELETE FROM chunks; DELETE FROM packs; DELETE FROM manifests;")?;
        Ok(())
    }

    // --- MinHash bands (parent detection) --------------------------------

    pub fn upsert_bands(&self, bucket: &str, keyhash: &str, bands: &[u64]) -> Result<()> {
        db_fault()?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut del =
                tx.prepare_cached("DELETE FROM manifest_bands WHERE bucket = ?1 AND keyhash = ?2")?;
            del.execute(rusqlite::params![bucket, keyhash])?;
            let mut ins = tx.prepare_cached(
                "INSERT OR IGNORE INTO manifest_bands (band, bucket, keyhash) VALUES (?1, ?2, ?3)",
            )?;
            for b in bands {
                ins.execute(rusqlite::params![*b as i64, bucket, keyhash])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_bands(&self, bucket: &str, keyhash: &str) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM manifest_bands WHERE bucket = ?1 AND keyhash = ?2",
            rusqlite::params![bucket, keyhash],
        )?;
        Ok(())
    }

    /// Keyhashes sharing at least one LSH band, most-sharing first.
    pub fn band_candidates(
        &self,
        bucket: &str,
        bands: &[u64],
        limit: usize,
    ) -> Result<Vec<String>> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let placeholders = vec!["?"; bands.len()].join(",");
        let sql = format!(
            "SELECT keyhash, COUNT(*) AS shared FROM manifest_bands
             WHERE bucket = ? AND band IN ({placeholders})
             GROUP BY keyhash ORDER BY shared DESC LIMIT ?"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(bucket.to_string())];
        for b in bands {
            params.push(Box::new(*b as i64));
        }
        params.push(Box::new(limit as i64));
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- Trained dictionaries (TrailStore) -------------------------------

    pub fn insert_dict(&self, dict_id: u64, bytes: &[u8]) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO dicts (dict_id, bytes) VALUES (?1, ?2)",
            rusqlite::params![dict_id as i64, bytes],
        )?;
        Ok(())
    }

    pub fn get_dict(&self, dict_id: u64) -> Result<Option<Vec<u8>>> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT bytes FROM dicts WHERE dict_id = ?1",
            [dict_id as i64],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e.into()),
        })
    }

    pub fn upsert_dict_rule(&self, bucket: &str, prefix: &str, dict_id: u64) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO dict_rules (bucket, prefix, dict_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![bucket, prefix, dict_id as i64],
        )?;
        Ok(())
    }

    /// Longest-prefix dict rule matching a key.
    pub fn dict_rule_for_key(&self, bucket: &str, key: &str) -> Result<Option<u64>> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare_cached("SELECT prefix, dict_id FROM dict_rules WHERE bucket = ?1")?;
        let rows = stmt
            .query_map([bucket], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter(|(p, _)| key.starts_with(p.as_str()))
            .max_by_key(|(p, _)| p.len())
            .map(|(_, id)| id))
    }

    pub fn all_dict_rules(&self, bucket: &str) -> Result<Vec<(String, u64)>> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare_cached("SELECT prefix, dict_id FROM dict_rules WHERE bucket = ?1")?;
        let rows = stmt
            .query_map([bucket], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // --- NVMe chunk cache accounting -----------------------------------

    pub fn cache_touch(
        &self,
        hash: &ChunkHash,
        size: u64,
        now_ms: u64,
        pinned: bool,
    ) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO cache_entries (hash, size, last_used_ms, pinned)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(hash) DO UPDATE SET
                last_used_ms = ?3,
                pinned = MAX(pinned, ?4)",
            rusqlite::params![
                hash.as_bytes().as_slice(),
                size as i64,
                now_ms as i64,
                pinned as i64
            ],
        )?;
        Ok(())
    }

    pub fn cache_total(&self) -> Result<u64> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let v: i64 = conn.query_row(
            "SELECT COALESCE(SUM(size), 0) FROM cache_entries",
            [],
            |r| r.get(0),
        )?;
        Ok(v as u64)
    }

    /// Oldest unpinned entries, for eviction.
    pub fn cache_eviction_candidates(&self, limit: usize) -> Result<Vec<(ChunkHash, u64)>> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT hash, size FROM cache_entries WHERE pinned = 0
             ORDER BY last_used_ms ASC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit as i64], |r| {
                let raw: Vec<u8> = r.get(0)?;
                let arr: [u8; 32] = raw.try_into().unwrap_or([0u8; 32]);
                Ok((ChunkHash(arr), r.get::<_, i64>(1)? as u64))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn cache_remove(&self, hash: &ChunkHash) -> Result<()> {
        db_fault()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM cache_entries WHERE hash = ?1",
            [hash.as_bytes().as_slice()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(n: u64) -> ChunkHash {
        ChunkHash::of(&n.to_le_bytes())
    }

    #[test]
    fn counters_accumulate() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.sqlite")).unwrap();
        db.bump("op_put_object", 1).unwrap();
        db.bump("op_put_object", 2).unwrap();
        assert_eq!(db.counter("op_put_object").unwrap(), 3);
        assert_eq!(db.counter("missing").unwrap(), 0);
    }

    #[test]
    fn shadow_stream_dedups_across_records() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.sqlite")).unwrap();
        let chunks = vec![(h(1), 100, 50), (h(2), 200, 60)];
        db.record_shadow_stream(300, &chunks).unwrap();
        db.record_shadow_stream(300, &chunks).unwrap(); // same content again
        let s = db.snapshot().unwrap();
        assert_eq!(s.shadow_logical_bytes, 600);
        assert_eq!(s.shadow_unique_raw_bytes, 300);
        assert_eq!(s.shadow_unique_stored_bytes, 110);
        assert_eq!(s.shadow_chunk_count, 4);
        assert_eq!(s.shadow_unique_chunk_count, 2);
        assert!((s.shadow_srf() - 600.0 / 110.0).abs() < 1e-9);
    }
}
