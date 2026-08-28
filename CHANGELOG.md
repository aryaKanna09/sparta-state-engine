# CHANGELOG

## M0 — Core library (2026-08-27)

`sparta-core` complete and green (fmt, clippy -D warnings, 49 tests):

- **Chunking**: FastCDC v2020 at spec parameters (256 KiB / 1 MiB / 4 MiB),
  slice and streaming APIs, with tests for coverage, determinism, size bounds,
  and boundary stability under small insertions.
- **Hashing**: BLAKE3 256-bit `ChunkHash` with hex serde and streaming hasher.
- **Compression normalization**: magic-byte detection for gzip/zstd/xz/zip;
  verified normalization for gzip (verbatim header + probed deflate level) and
  zstd (probed level × checksum × content-size grid) with streaming byte-exact
  comparison and early abort; anything unverifiable is stored opaque.
  Decompression-bomb guard. xz/zip detected but opaque (DECISIONS.md D-003).
- **Packfile format v1**: self-describing records + embedded tail index +
  footer; per-chunk zstd-3 with raw fallback for incompressible chunks;
  within-pack dedup; recovery scan that ignores the index; corruption tests.
- **Manifest format v1**: ordered chunk refs with pack locations, original
  metadata, recompression recipe, lineage pointer, MinHash signature;
  structural validation.
- **MinHash**: 128-permutation sketch with fixed seeds, Jaccard estimation,
  32×4 LSH banding.
- **SRF accounting**: shared dedup accumulator used by scanner and benchmarks.
- **Property test**: arbitrary payloads (3 entropy profiles × sizes crossing
  chunk boundaries × {plain, gzip, zstd} wrapping) through normalize → chunk →
  pack → reconstruct → recompress, asserting byte identity and hash
  verification end to end.

Workspace scaffolding, CI (fmt/clippy/test + MinIO integration lane), SPEC.md,
DECISIONS.md D-001..D-008.
