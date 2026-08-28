//! NVMe-backed LRU chunk cache with pinning.
//!
//! Chunks are cached as individual files named by their BLAKE3 hex under the
//! cache directory. LRU accounting lives in the local DB (cache metadata is
//! disposable — a lost cache is only a cold start). Pinned chunks (base
//! template data, per `SPARTA_PIN_PREFIXES`) are never evicted.
//!
//! Every failure path degrades to "cache miss": the cache can slow the engine
//! down, never break it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{debug, warn};

use sparta_core::hash::ChunkHash;

use crate::db::Db;

pub struct ChunkCache {
    dir: PathBuf,
    capacity: u64,
    db: Arc<Db>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ChunkCache {
    /// Returns None (cache disabled) if the directory can't be prepared.
    pub fn open(dir: &str, capacity: u64, db: Arc<Db>) -> Option<Self> {
        let dir = PathBuf::from(dir);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            warn!(dir = %dir.display(), error = %e, "chunk cache disabled");
            return None;
        }
        Some(Self { dir, capacity, db })
    }

    fn path_for(&self, hash: &ChunkHash) -> PathBuf {
        self.dir.join(hash.to_hex())
    }

    /// Raw (decompressed) chunk bytes, if cached. Verifies the hash so a
    /// corrupted cache file is treated as a miss and removed.
    pub fn get(&self, hash: &ChunkHash) -> Option<Vec<u8>> {
        let path = self.path_for(hash);
        let data = std::fs::read(&path).ok()?;
        if ChunkHash::of(&data) != *hash {
            warn!(chunk = %hash, "corrupt cache entry; dropping");
            let _ = std::fs::remove_file(&path);
            let _ = self.db.cache_remove(hash);
            return None;
        }
        let _ = self
            .db
            .cache_touch(hash, data.len() as u64, now_ms(), false);
        Some(data)
    }

    /// Insert (best effort); evicts LRU entries beyond capacity.
    pub fn put(&self, hash: &ChunkHash, data: &[u8], pinned: bool) {
        let path = self.path_for(hash);
        let tmp = path.with_extension("tmp");
        let write = std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, &path));
        if let Err(e) = write {
            debug!(chunk = %hash, error = %e, "cache write failed");
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        let _ = self
            .db
            .cache_touch(hash, data.len() as u64, now_ms(), pinned);
        self.evict_to_capacity();
    }

    fn evict_to_capacity(&self) {
        let Ok(mut total) = self.db.cache_total() else {
            return;
        };
        while total > self.capacity {
            let Ok(candidates) = self.db.cache_eviction_candidates(32) else {
                return;
            };
            if candidates.is_empty() {
                return; // everything pinned
            }
            for (hash, size) in candidates {
                if total <= self.capacity {
                    break;
                }
                let _ = std::fs::remove_file(self.path_for(&hash));
                let _ = self.db.cache_remove(&hash);
                total = total.saturating_sub(size);
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Should this object's chunks be pinned? Matches key against the
/// comma-separated `SPARTA_PIN_PREFIXES` list.
pub fn should_pin(pin_prefixes: &[String], key: &str) -> bool {
    pin_prefixes
        .iter()
        .any(|p| !p.is_empty() && key.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(capacity: u64) -> (tempfile::TempDir, ChunkCache) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(&dir.path().join("db.sqlite")).unwrap());
        let cache =
            ChunkCache::open(dir.path().join("cache").to_str().unwrap(), capacity, db).unwrap();
        (dir, cache)
    }

    #[test]
    fn roundtrip_and_verify() {
        let (_g, cache) = setup(1 << 20);
        let data = vec![7u8; 1000];
        let h = ChunkHash::of(&data);
        assert!(cache.get(&h).is_none());
        cache.put(&h, &data, false);
        assert_eq!(cache.get(&h).unwrap(), data);
    }

    #[test]
    fn corrupt_entry_is_a_miss() {
        let (_g, cache) = setup(1 << 20);
        let data = vec![1u8; 500];
        let h = ChunkHash::of(&data);
        cache.put(&h, &data, false);
        std::fs::write(cache.dir().join(h.to_hex()), b"garbage").unwrap();
        assert!(cache.get(&h).is_none());
    }

    #[test]
    fn lru_eviction_spares_pinned() {
        let (_g, cache) = setup(2500);
        let mk = |seed: u8| {
            let data = vec![seed; 1000];
            (ChunkHash::of(&data), data)
        };
        let (h1, d1) = mk(1);
        let (h2, d2) = mk(2);
        let (h3, d3) = mk(3);
        cache.put(&h1, &d1, true); // pinned
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache.put(&h2, &d2, false);
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache.put(&h3, &d3, false); // total 3000 > 2500: evict oldest unpinned = h2
        assert!(cache.get(&h1).is_some(), "pinned must survive");
        assert!(cache.get(&h2).is_none(), "oldest unpinned must be evicted");
        assert!(cache.get(&h3).is_some());
    }

    #[test]
    fn pin_prefix_match() {
        let prefixes = vec!["templates/".to_string()];
        assert!(should_pin(&prefixes, "templates/base.img"));
        assert!(!should_pin(&prefixes, "snapshots/run1.img"));
        assert!(!should_pin(&[String::new()], "anything"));
    }
}
