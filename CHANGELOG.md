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

## M1 — Scanner / Shadow A (2026-08-27)

`sparta-scanner`: standalone read-only redundancy scanner.

- Enumeration via S3 Inventory (CSV manifest, local or s3://, gzip data files)
  with ListObjectsV2 fallback; multi-bucket.
- Seeded uniform sampling under a global byte budget with a per-object fetch
  cap (large objects fingerprinted on a prefix window, opaque).
- Fingerprinting with the full spec pipeline (normalization + FastCDC +
  BLAKE3), cross-object stored-size cache, concurrent fetch + spawn_blocking
  CPU work.
- SRF-S point estimate with exact leave-one-out jackknife 95% CI (D-009);
  with/without-normalization comparison with a >3x reverification flag;
  addressable SRF via prefix-glob and tag exclusion rules; breakdowns by
  prefix and content type; MinHash LSH redundancy clusters.
- Reports: JSON + self-contained single-file HTML (no scripts, no external
  assets).
- Testkit: MinIO server harness (native binary, D-002), deterministic fleet
  fixture generator with localized-hotspot mutation (D-010), bucket seeding
  helpers, `sparta-fixtures` CLI.
- Acceptance: end-to-end MinIO integration test (SRF, CI bracketing,
  normalization, exclusions, clusters, HTML). Scale run: 100,000-object /
  6.25 GiB bucket scanned at 1% sampling in 28.5 s wall clock (target: <1 h).

## M2 — Gateway passthrough and Shadow B (2026-08-27)

`sparta-gateway`: full S3 surface via s3s/s3s-aws with Invariant 0.

- All 107 S3 operations the upstream proxy supports delegate transparently
  (literal generated delegation, D-017); PUT/GET/UploadPart intercepted for
  shadow observation only.
- Shadow taps: incremental spec-parameter CDC on streamed bodies (byte-
  identical to one-shot chunking, bounded memory), forwarding untouched;
  panic-disarmed per stream; stats flushed on end-of-stream or drop.
- Invariant 0: panic nets on every handler; `SPARTA_BYPASS=1` plus
  `SPARTA_BYPASS_FILE` for restart-free per-request bypass (D-012);
  `/healthz` + `/readyz` with a background upstream reachability probe;
  configurable upstream retries/attempt timeout (D-016).
- Local SQLite (WAL) shadow stats: op counts, bytes in/out, dedup-only shadow
  SRF lower bound (D-013/D-014); `sparta-gateway stats` prints JSON.
- Testkit: fault-injecting HTTP proxy (500s, connection drops, delays, hard
  outage), fault hooks for dead-DB and panicking-chunker (D-015), boto3 and
  aws-CLI workflow scripts.
- Integration suite (CI merge gate): boto3 workflow (multipart, list, ranged
  GET, multipart copy, delete, SHA-256 verified) clean and with all fault
  classes active at once; aws CLI workflow; per-fault matrix; bypass-file
  toggling; readyz failover; passthrough overhead sanity check.
