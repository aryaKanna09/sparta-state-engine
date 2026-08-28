//! Meter record format and the tamper-evident hash chain.
//!
//! Records serialize as single-line JSON. The chain hash of a record is
//! `BLAKE3(prev_hash_hex || canonical_json_of_record_with_empty_hash)`;
//! the genesis record chains from 64 zero hex digits. Field order is fixed by
//! the struct definitions (serde_json preserves declaration order), which
//! makes the serialized form canonical for hashing.

use serde::{Deserialize, Serialize};

pub const GENESIS_PREV: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Which pipeline produced the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Shadow,
    Active,
}

/// One entry in the meter log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeterRecord {
    pub seq: u64,
    pub ts_ms: u64,
    pub mode: Mode,
    #[serde(flatten)]
    pub body: RecordBody,
    /// Chain hash of the previous record (hex), genesis = all zeros.
    pub prev: String,
    /// This record's chain hash (hex); empty while computing.
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordBody {
    /// One client-facing data operation.
    Op {
        op: String,
        bucket: String,
        /// Bytes as the client sees them (uploaded or downloaded).
        logical_bytes: u64,
        /// Bytes the engine actually moved to/from the upstream for the
        /// client payload (== logical in shadow/passthrough).
        physical_bytes: u64,
        /// Upstream PUT-class requests the engine spent on its own
        /// artifacts (pack/manifest/index writes) for this operation.
        engine_put_requests: u64,
        /// Upstream GET-class requests the engine spent (pack reads).
        engine_get_requests: u64,
        /// Engine artifact bytes moved upstream for this operation.
        engine_bytes: u64,
        /// CPU time the engine spent on this operation (chunking, hashing,
        /// compression), microseconds.
        engine_cpu_us: u64,
    },
    /// Periodic storage gauge: what is stored right now.
    Level {
        /// Sum of logical object sizes currently live through the engine.
        logical_stored_bytes: u64,
        /// Physical bytes the engine keeps for them (packs + manifests).
        physical_stored_bytes: u64,
        /// True when levels are projections (shadow mode) rather than
        /// metered actuals (active mode).
        projected: bool,
    },
    /// A garbage-collection sweep (every sweep is logged; spec requirement).
    GcSweep {
        packs_deleted: u64,
        bytes_freed: u64,
        chunks_freed: u64,
        dry_run: bool,
    },
    /// Free-form operational note (mode changes, rebuilds, config).
    Note { text: String },
}

impl MeterRecord {
    /// Compute the chain hash for this record given the previous hash.
    /// `self.hash` is ignored (treated as empty) during computation.
    pub fn compute_hash(&self, prev_hash: &str) -> String {
        let mut clone = self.clone();
        clone.prev = prev_hash.to_string();
        clone.hash = String::new();
        let payload = serde_json::to_string(&clone).expect("record serializes");
        let mut hasher = blake3::Hasher::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(payload.as_bytes());
        hex::encode(hasher.finalize().as_bytes())
    }

    /// Seal the record onto the chain: sets `prev` and `hash`.
    pub fn seal(mut self, prev_hash: &str) -> Self {
        self.hash = self.compute_hash(prev_hash);
        self.prev = prev_hash.to_string();
        self
    }

    /// Verify this record's hash against its recorded `prev`.
    pub fn verify_against_prev(&self) -> bool {
        self.compute_hash(&self.prev) == self.hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(seq: u64) -> MeterRecord {
        MeterRecord {
            seq,
            ts_ms: 1_756_000_000_000 + seq,
            mode: Mode::Shadow,
            body: RecordBody::Op {
                op: "put_object".into(),
                bucket: "fleet".into(),
                logical_bytes: 100,
                physical_bytes: 100,
                engine_put_requests: 0,
                engine_get_requests: 0,
                engine_bytes: 0,
                engine_cpu_us: 42,
            },
            prev: String::new(),
            hash: String::new(),
        }
    }

    #[test]
    fn seal_and_verify() {
        let r = op(0).seal(GENESIS_PREV);
        assert_eq!(r.prev, GENESIS_PREV);
        assert!(r.verify_against_prev());
    }

    #[test]
    fn tamper_detected() {
        let r = op(0).seal(GENESIS_PREV);
        let mut tampered = r.clone();
        if let RecordBody::Op { logical_bytes, .. } = &mut tampered.body {
            *logical_bytes = 1; // shrink the metered bytes
        }
        assert!(!tampered.verify_against_prev());
    }

    #[test]
    fn chain_links() {
        let a = op(0).seal(GENESIS_PREV);
        let b = op(1).seal(&a.hash);
        assert!(b.verify_against_prev());
        // Replacing a breaks b's linkage.
        let a2 = {
            let mut x = op(0);
            x.ts_ms += 1;
            x.seal(GENESIS_PREV)
        };
        assert_ne!(a2.hash, a.hash);
        assert_ne!(b.prev, a2.hash);
    }

    #[test]
    fn json_roundtrip() {
        let r = op(7).seal(GENESIS_PREV);
        let line = serde_json::to_string(&r).unwrap();
        let back: MeterRecord = serde_json::from_str(&line).unwrap();
        assert_eq!(back, r);
        assert!(back.verify_against_prev());
    }
}
