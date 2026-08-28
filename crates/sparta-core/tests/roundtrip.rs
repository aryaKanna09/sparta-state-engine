//! End-to-end property test for M0: for arbitrary inputs — including gzip- and
//! zstd-wrapped ones — normalize, chunk, pack, then reconstruct from packs +
//! manifest alone, and assert byte identity with the original.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use proptest::prelude::*;

use sparta_core::chunker::chunk_spans;
use sparta_core::compression::{normalize, reproduce, SniffOutcome};
use sparta_core::hash::ChunkHash;
use sparta_core::manifest::{ChunkRef, Manifest};
use sparta_core::packfile::{decode_chunk, read_index, PackBuilder, SealedPack};
use sparta_core::spec;

const MAX_RAW: u64 = 1 << 30;

/// The full write path in memory: returns (manifest, packs by id).
fn ingest(
    bucket: &str,
    key: &str,
    original: &[u8],
    seal_every: usize,
) -> (Manifest, HashMap<String, SealedPack>) {
    let outcome = normalize(original, true, MAX_RAW);
    let (payload, recipe, detected): (Vec<u8>, _, _) = match outcome {
        SniffOutcome::Normalized {
            raw,
            recipe,
            detected,
        } => (raw, Some(recipe), Some(detected)),
        SniffOutcome::Opaque { detected } => (original.to_vec(), None, detected),
    };

    let spans = chunk_spans(&payload);
    let mut packs: HashMap<String, SealedPack> = HashMap::new();
    // Where each distinct chunk landed once its pack sealed.
    let mut placed: HashMap<ChunkHash, ChunkRef> = HashMap::new();

    let mut builder = PackBuilder::new();
    let mut pending: Vec<sparta_core::packfile::PackEntry> = Vec::new();

    fn seal_now(
        builder: &mut PackBuilder,
        pending: &mut Vec<sparta_core::packfile::PackEntry>,
        placed: &mut HashMap<ChunkHash, ChunkRef>,
        packs: &mut HashMap<String, SealedPack>,
    ) {
        if builder.is_empty() {
            return;
        }
        let sealed = std::mem::take(builder).seal();
        for entry in pending.drain(..) {
            placed.insert(
                entry.hash,
                ChunkRef {
                    hash: entry.hash,
                    raw_len: entry.raw_len,
                    pack_id: sealed.pack_id.clone(),
                    offset: entry.offset,
                    data_len: entry.data_len,
                    codec: entry.codec,
                    dict_id: entry.dict_id,
                    dict: None,
                },
            );
        }
        packs.insert(sealed.pack_id.clone(), sealed);
    }

    for span in &spans {
        if placed.contains_key(&span.hash) || pending.iter().any(|e| e.hash == span.hash) {
            continue; // dedup hit: already stored or queued in the open pack
        }
        let data = &payload[span.offset..span.offset + span.length];
        let entry = builder.add_chunk(span.hash, data).expect("add chunk");
        pending.push(entry);
        if builder.is_full() || (seal_every > 0 && pending.len() >= seal_every) {
            seal_now(&mut builder, &mut pending, &mut placed, &mut packs);
        }
    }
    seal_now(&mut builder, &mut pending, &mut placed, &mut packs);

    // Manifest chunk list in payload order; identical hashes imply identical
    // bytes, so every span resolves through `placed`.
    let ordered: Vec<ChunkRef> = spans
        .iter()
        .map(|s| placed.get(&s.hash).expect("placed").clone())
        .collect();

    let manifest = Manifest {
        format_version: spec::FORMAT_VERSION,
        bucket: bucket.into(),
        key: key.into(),
        version_id: None,
        logical_size: original.len() as u64,
        object_hash: ChunkHash::of(original),
        content_type: None,
        user_metadata: BTreeMap::new(),
        etag: None,
        detected_compression: detected.map(|k| k.as_str().to_string()),
        recipe,
        payload_size: payload.len() as u64,
        payload_hash: ChunkHash::of(&payload),
        chunks: ordered,
        parent: None,
        minhash: None,
        created_unix_ms: 0,
        quarantined: false,
    };
    manifest.validate().expect("manifest validates");
    (manifest, packs)
}

/// The full read path: reconstruct the object from manifest + packs only.
fn restore(manifest: &Manifest, packs: &HashMap<String, SealedPack>) -> Vec<u8> {
    let mut payload = Vec::with_capacity(manifest.payload_size as usize);
    for r in &manifest.chunks {
        let pack = packs.get(&r.pack_id).expect("pack present");
        // Simulate a ranged GET.
        let data = &pack.bytes[r.offset as usize..(r.offset + r.data_len) as usize];
        let entry = sparta_core::packfile::PackEntry {
            hash: r.hash,
            offset: r.offset,
            data_len: r.data_len,
            raw_len: r.raw_len,
            codec: r.codec,
            dict_id: r.dict_id,
        };
        let raw = decode_chunk(&entry, data, None).expect("chunk decodes and verifies");
        payload.extend_from_slice(&raw);
    }
    assert_eq!(
        ChunkHash::of(&payload),
        manifest.payload_hash,
        "payload hash must verify"
    );
    let object = match &manifest.recipe {
        Some(recipe) => reproduce(recipe, &payload).expect("recipe reproduces"),
        None => payload,
    };
    assert_eq!(
        ChunkHash::of(&object),
        manifest.object_hash,
        "object hash must verify before serving"
    );
    assert_eq!(object.len() as u64, manifest.logical_size);
    object
}

fn assert_roundtrip(original: &[u8], seal_every: usize) {
    let (manifest, packs) = ingest("bkt", "key", original, seal_every);
    let restored = restore(&manifest, &packs);
    assert_eq!(
        restored, original,
        "byte identity through the full pipeline"
    );
    // Every pack's embedded index must parse and match its entries.
    for p in packs.values() {
        assert_eq!(read_index(&p.bytes).unwrap(), p.entries);
    }
}

fn gzip_wrap(raw: &[u8], level: u32) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(level));
    enc.write_all(raw).unwrap();
    enc.finish().unwrap()
}

fn zstd_wrap(raw: &[u8], level: i32) -> Vec<u8> {
    zstd::stream::encode_all(raw, level).unwrap()
}

// --- deterministic cases -------------------------------------------------

#[test]
fn empty_object() {
    assert_roundtrip(&[], 0);
}

#[test]
fn tiny_object() {
    assert_roundtrip(b"tiny", 0);
}

#[test]
fn multi_chunk_random_data() {
    let mut data = vec![0u8; 9 * 1024 * 1024 + 137];
    let mut s = 0xdeadbeefu64;
    for b in data.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *b = s as u8;
    }
    assert_roundtrip(&data, 0);
    assert_roundtrip(&data, 2); // force multiple packs
}

#[test]
fn duplicate_heavy_data_dedups_across_packs() {
    // Same 2 MiB block repeated: later references must point into the first
    // pack that stored it, across seal boundaries.
    let block: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let mut data = Vec::new();
    for _ in 0..6 {
        data.extend_from_slice(&block);
    }
    let (manifest, packs) = ingest("b", "k", &data, 1);
    let restored = restore(&manifest, &packs);
    assert_eq!(restored, data);
    let unique: std::collections::HashSet<_> = manifest.chunks.iter().map(|c| c.hash).collect();
    assert!(
        unique.len() < manifest.chunks.len(),
        "repeats must dedup: {} unique of {}",
        unique.len(),
        manifest.chunks.len()
    );
}

#[test]
fn gzipped_object_roundtrips_through_normalization() {
    let raw: Vec<u8> = (0..3 * 1024 * 1024)
        .map(|i| ((i / 100) % 251) as u8)
        .collect();
    for level in [1u32, 6, 9] {
        let original = gzip_wrap(&raw, level);
        let (manifest, packs) = ingest("b", "k", &original, 0);
        assert!(
            manifest.recipe.is_some(),
            "gzip level {level} should normalize"
        );
        assert_eq!(restore(&manifest, &packs), original);
    }
}

#[test]
fn zstd_object_roundtrips_through_normalization() {
    let raw: Vec<u8> = (0..2 * 1024 * 1024)
        .map(|i| ((i / 300) % 250) as u8)
        .collect();
    for level in [1i32, 3, 12] {
        let original = zstd_wrap(&raw, level);
        let (manifest, packs) = ingest("b", "k", &original, 0);
        assert!(
            manifest.recipe.is_some(),
            "zstd level {level} should normalize"
        );
        assert_eq!(restore(&manifest, &packs), original);
    }
}

#[test]
fn normalization_finds_shared_chunks_across_gzip_levels() {
    // The entire point: the same raw payload compressed at different levels
    // produces disjoint compressed bytes but identical raw chunks.
    let raw: Vec<u8> = (0..6 * 1024 * 1024)
        .map(|i| ((i / 777) % 251) as u8)
        .collect();
    let (m1, _) = ingest("b", "k1", &gzip_wrap(&raw, 1), 0);
    let (m9, _) = ingest("b", "k9", &gzip_wrap(&raw, 9), 0);
    let h1: std::collections::HashSet<_> = m1.chunks.iter().map(|c| c.hash).collect();
    let h9: std::collections::HashSet<_> = m9.chunks.iter().map(|c| c.hash).collect();
    assert_eq!(h1, h9, "normalized chunk sets must be identical");
}

// --- property test -------------------------------------------------------

#[derive(Debug, Clone)]
enum Wrap {
    None,
    Gzip(u32),
    Zstd(i32),
}

fn payload_strategy() -> impl Strategy<Value = Vec<u8>> {
    // Mix of sizes crossing chunk boundaries, and of entropy profiles.
    let size = prop_oneof![
        0usize..1024,
        200_000usize..600_000,
        4_000_000usize..6_500_000,
    ];
    (size, any::<u64>(), 0u8..3).prop_map(|(len, seed, kind)| {
        let mut out = vec![0u8; len];
        let mut s = seed | 1;
        match kind {
            0 => {
                // pseudo-random (incompressible)
                for b in out.iter_mut() {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    *b = s as u8;
                }
            }
            1 => {
                // repetitive (highly compressible)
                for (i, b) in out.iter_mut().enumerate() {
                    *b = ((i / 64).wrapping_add(seed as usize) % 251) as u8;
                }
            }
            _ => {
                // mixed: random blocks interleaved with runs
                for (i, b) in out.iter_mut().enumerate() {
                    if (i / 4096) % 2 == 0 {
                        s ^= s << 13;
                        s ^= s >> 7;
                        s ^= s << 17;
                        *b = s as u8;
                    } else {
                        *b = (i % 17) as u8;
                    }
                }
            }
        }
        out
    })
}

fn wrap_strategy() -> impl Strategy<Value = Wrap> {
    prop_oneof![
        Just(Wrap::None),
        (1u32..=9).prop_map(Wrap::Gzip),
        prop_oneof![Just(1i32), Just(3), Just(8)].prop_map(Wrap::Zstd),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 12,
        max_shrink_iters: 40,
        .. ProptestConfig::default()
    })]

    #[test]
    fn arbitrary_inputs_roundtrip(
        payload in payload_strategy(),
        wrap in wrap_strategy(),
        seal_every in prop_oneof![Just(0usize), Just(2usize)],
    ) {
        let original = match wrap {
            Wrap::None => payload,
            Wrap::Gzip(level) => gzip_wrap(&payload, level),
            Wrap::Zstd(level) => zstd_wrap(&payload, level),
        };
        assert_roundtrip(&original, seal_every);
    }
}
