//! Append-only meter log on disk: one JSON record per line.
//!
//! The writer keeps the chain state (last seq + hash) and appends atomically
//! under a lock; reads stream the file and re-verify the chain. The log file
//! is an engine artifact: the gateway mirrors sealed byte ranges into the
//! customer's bucket under `_sparta/meter/` (wired in the gateway crate).

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::record::{MeterRecord, Mode, RecordBody, GENESIS_PREV};
use crate::MeterError;

pub struct MeterWriter {
    inner: Mutex<WriterState>,
    path: PathBuf,
}

struct WriterState {
    file: File,
    next_seq: u64,
    last_hash: String,
}

impl MeterWriter {
    /// Open (or create) a meter log, verifying the existing chain first so a
    /// corrupted log is caught at startup, not at billing time.
    pub fn open(path: &Path) -> Result<Self, MeterError> {
        let (next_seq, last_hash) = match verify_file(path) {
            Ok(summary) => (summary.record_count, summary.last_hash),
            Err(MeterError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                (0, GENESIS_PREV.to_string())
            }
            Err(e) => return Err(e),
        };
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.seek(SeekFrom::End(0))?;
        Ok(Self {
            inner: Mutex::new(WriterState {
                file,
                next_seq,
                last_hash,
            }),
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one record; assigns seq, chains, writes, and flushes.
    pub fn append(&self, ts_ms: u64, mode: Mode, body: RecordBody) -> Result<u64, MeterError> {
        let mut st = self.inner.lock().unwrap();
        let record = MeterRecord {
            seq: st.next_seq,
            ts_ms,
            mode,
            body,
            prev: String::new(),
            hash: String::new(),
        }
        .seal(&st.last_hash.clone());
        let line = serde_json::to_string(&record)?;
        st.file.write_all(line.as_bytes())?;
        st.file.write_all(b"\n")?;
        st.file.flush()?;
        st.last_hash = record.hash.clone();
        st.next_seq += 1;
        Ok(record.seq)
    }

    /// Durability point: fsync the log (called on flush intervals/shutdown).
    pub fn sync(&self) -> Result<(), MeterError> {
        let st = self.inner.lock().unwrap();
        st.file.sync_data()?;
        Ok(())
    }

    pub fn last_hash(&self) -> String {
        self.inner.lock().unwrap().last_hash.clone()
    }
}

/// Result of a successful chain verification.
#[derive(Debug)]
pub struct VerifySummary {
    pub record_count: u64,
    pub last_hash: String,
    pub first_ts_ms: Option<u64>,
    pub last_ts_ms: Option<u64>,
}

/// Stream and verify the whole chain; error pinpoints the first bad record.
pub fn verify_file(path: &Path) -> Result<VerifySummary, MeterError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut expected_prev = GENESIS_PREV.to_string();
    let mut count = 0u64;
    let mut first_ts = None;
    let mut last_ts = None;
    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let rec: MeterRecord =
            serde_json::from_str(&line).map_err(|e| MeterError::ChainBroken {
                seq: i as u64,
                reason: format!("unparseable record: {e}"),
            })?;
        if rec.seq != count {
            return Err(MeterError::ChainBroken {
                seq: count,
                reason: format!("sequence gap: found seq {}", rec.seq),
            });
        }
        if rec.prev != expected_prev {
            return Err(MeterError::ChainBroken {
                seq: rec.seq,
                reason: "prev hash does not match preceding record".into(),
            });
        }
        if !rec.verify_against_prev() {
            return Err(MeterError::ChainBroken {
                seq: rec.seq,
                reason: "record hash invalid (contents modified)".into(),
            });
        }
        expected_prev = rec.hash.clone();
        first_ts.get_or_insert(rec.ts_ms);
        last_ts = Some(rec.ts_ms);
        count += 1;
    }
    Ok(VerifySummary {
        record_count: count,
        last_hash: expected_prev,
        first_ts_ms: first_ts,
        last_ts_ms: last_ts,
    })
}

/// Load all records (verifying the chain).
pub fn read_all(path: &Path) -> Result<Vec<MeterRecord>, MeterError> {
    verify_file(path)?; // fail fast on tampering
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str(&line)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op_body(bytes: u64) -> RecordBody {
        RecordBody::Op {
            op: "put_object".into(),
            bucket: "b".into(),
            logical_bytes: bytes,
            physical_bytes: bytes,
            engine_put_requests: 0,
            engine_get_requests: 0,
            engine_bytes: 0,
            engine_cpu_us: 10,
        }
    }

    #[test]
    fn append_reopen_verify() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meter.jsonl");
        {
            let w = MeterWriter::open(&path).unwrap();
            for i in 0..10 {
                w.append(1000 + i, Mode::Shadow, op_body(i)).unwrap();
            }
            w.sync().unwrap();
        }
        // Reopen continues the chain.
        {
            let w = MeterWriter::open(&path).unwrap();
            let seq = w.append(2000, Mode::Shadow, op_body(99)).unwrap();
            assert_eq!(seq, 10);
        }
        let summary = verify_file(&path).unwrap();
        assert_eq!(summary.record_count, 11);
    }

    #[test]
    fn tampering_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meter.jsonl");
        let w = MeterWriter::open(&path).unwrap();
        for i in 0..5 {
            w.append(1000 + i, Mode::Shadow, op_body(1000)).unwrap();
        }
        drop(w);
        // Shrink a metered byte count in the middle of the file.
        let content = std::fs::read_to_string(&path).unwrap();
        let tampered = content.replacen("\"logical_bytes\":1000", "\"logical_bytes\":1", 1);
        assert_ne!(content, tampered);
        std::fs::write(&path, tampered).unwrap();
        let err = verify_file(&path).unwrap_err();
        match err {
            MeterError::ChainBroken { reason, .. } => {
                assert!(reason.contains("hash invalid"), "{reason}")
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    #[test]
    fn record_deletion_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meter.jsonl");
        let w = MeterWriter::open(&path).unwrap();
        for i in 0..5 {
            w.append(1000 + i, Mode::Shadow, op_body(i)).unwrap();
        }
        drop(w);
        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines: Vec<&str> = content.lines().collect();
        lines.remove(2);
        std::fs::write(&path, lines.join("\n")).unwrap();
        assert!(verify_file(&path).is_err());
    }

    #[test]
    fn truncation_from_end_still_verifies_but_shortens() {
        // Chopping the tail is indistinguishable from "no more records yet";
        // the defense is the mirrored last_hash in the bucket (gateway layer).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meter.jsonl");
        let w = MeterWriter::open(&path).unwrap();
        for i in 0..5 {
            w.append(1000 + i, Mode::Shadow, op_body(i)).unwrap();
        }
        drop(w);
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        std::fs::write(&path, lines[..3].join("\n")).unwrap();
        let s = verify_file(&path).unwrap();
        assert_eq!(s.record_count, 3);
    }
}
