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
