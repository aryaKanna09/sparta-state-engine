//! Naming for engine artifacts under the `_sparta/` prefix.
//!
//! Everything the engine stores lives in the customer's own bucket under this
//! prefix, using the customer's encryption settings. Manifests are addressed
//! by the BLAKE3 of the object key (keys can be arbitrary UTF-8 up to 1 KiB;
//! the real key lives inside the manifest JSON).

use sparta_core::hash::ChunkHash;

pub const SPARTA_PREFIX: &str = "_sparta/";
pub const MANIFEST_PREFIX: &str = "_sparta/manifests/";
pub const PACK_PREFIX: &str = "_sparta/packs/";
pub const GC_CANDIDATE_PREFIX: &str = "_sparta/gc/candidates/";
/// Reserved for the meter-log mirroring path.
#[allow(dead_code)]
pub const METER_PREFIX: &str = "_sparta/meter/";

pub fn keyhash(key: &str) -> String {
    ChunkHash::of(key.as_bytes()).to_hex()
}

pub fn manifest_key(kh: &str) -> String {
    format!("{MANIFEST_PREFIX}{kh}.json")
}

pub fn pack_key(pack_id: &str) -> String {
    format!("{PACK_PREFIX}{pack_id}.pack")
}

pub fn gc_candidate_key(pack_id: &str) -> String {
    format!("{GC_CANDIDATE_PREFIX}{pack_id}.json")
}

/// Extract the pack id from a `_sparta/packs/...` key.
pub fn pack_id_from_key(key: &str) -> Option<&str> {
    key.strip_prefix(PACK_PREFIX)?.strip_suffix(".pack")
}

/// Extract the keyhash from a `_sparta/manifests/...` key.
pub fn keyhash_from_manifest_key(key: &str) -> Option<&str> {
    key.strip_prefix(MANIFEST_PREFIX)?.strip_suffix(".json")
}

/// True for keys the gateway must hide from client listings.
pub fn is_engine_key(key: &str) -> bool {
    key.starts_with(SPARTA_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips() {
        let kh = keyhash("snapshots/tpl0/sandbox-0001.tar");
        assert_eq!(kh.len(), 64);
        assert_eq!(
            keyhash_from_manifest_key(&manifest_key(&kh)),
            Some(kh.as_str())
        );
        assert_eq!(pack_id_from_key(&pack_key("ab12")), Some("ab12"));
        assert!(is_engine_key(&manifest_key(&kh)));
        assert!(!is_engine_key("snapshots/x.tar"));
    }
}
