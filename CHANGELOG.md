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

## M3 — Meter (2026-08-27)

`sparta-meter`: the tamper-evident metering log, pricing, and reporting.

- Append-only JSONL log; every record BLAKE3-chained over its predecessor;
  `sparta-meter verify` revalidates the chain and pinpoints the first bad
  record. Unit tests prove edit/deletion detection and truncation semantics.
- Record kinds: per-operation logical/physical bytes + engine request/byte/CPU
  overheads, storage Level gauges (with `projected` labeling), GC sweeps,
  operational notes (D-018/D-019).
- Pricing: built-in AWS S3 Standard tiered rate card; CUR loader deriving the
  customer's effective storage/request/egress rates (D-021).
- Reports: NET savings (gross tiered-storage savings minus engine request and
  compute costs), VSRR trailing-30-day annualized (D-020); monthly invoice
  draft derived only from the log; CSV and Parquet exports (round-trip tested).
- Gateway wiring: shadow taps append Op records (measured chunking CPU) and an
  hourly projected Level gauge to the same log active mode will use; meter
  failures can never fail a request. Integration test runs the boto3 workflow
  then `sparta-meter verify` against the produced log.

## M4 — Activation: write-side dedup (2026-08-28)

Active mode (`SPARTA_MODE=active`): the engine becomes the storage path for
newly written objects, with Invariant 0 intact end to end.

- **Write-through ingest** (D-022): client writes pass through raw (identical
  ack semantics to S3, single PUT and multipart alike), then a background
  worker ingests in order — sniff/normalize/chunk/dedup into shared 48 MB
  packs, manifests, then raw deletion. Crash anywhere = raw stays, no loss.
- **Restore path**: manifest-first GETs with per-chunk cache lookups,
  adjacent-range coalescing into single ranged GETs (≤32 MB groups), bounded
  parallel prefetch, streaming reconstruction; full GETs verify the object
  BLAKE3 before the final byte (hold-back frame), recipe objects are rebuilt
  and re-verified post-recompression; mismatch = CRITICAL log + quarantine +
  raw fallback (D-024). Ranged GETs serve chunk-verified subranges.
- **NVMe LRU chunk cache** with hash verification on read, atomic writes, and
  pinning via `SPARTA_PIN_PREFIXES`.
- **Full data plane for engine objects**: HEAD from manifests (sizes, ETags,
  content type, user metadata), DELETE (manifest + raw + queue), same-bucket
  COPY as manifest clone (zero data movement), cross-bucket COPY by
  materialization, merged ListObjectsV2 (upstream ∪ manifests, `_sparta/`
  hidden, engine wins collisions, delimiter grouping, combined continuation
  tokens, D-027).
- **GC** (D-026): chunk-refcounted mark → garbage-fraction compaction
  (rewrites packs, updates manifests) → two-phase sweep with bucket candidate
  markers and `SPARTA_GC_GRACE_DAYS`; `--dry-run` reports all phases; every
  sweep logged to the meter.
- **rebuild-index**: full local DB reconstruction from bucket manifests alone.
- Acceptance (all green against MinIO): lifecycle test (recipe path, ranges,
  metadata, listing, copy, delete); **fleet SRF 6.89× (target ≥5)** on the
  200-snapshot/3-template fixture incl. gzip variants; boto3 workflow in
  active mode; byte identity after rebuild-index on a deleted DB; GC dry-run
  + real two-phase run shrinking pack bytes with survivors intact; fault
  suite incl. dead-DB + panicking chunker in active mode.

## M5 — Ratio work (2026-08-28)

- **Checkpoint-aware chunking**: safetensors (JSON header tensor offsets) and
  PyTorch zip (central directory entry offsets) boundary detection; chunks cut
  at tensor/entry boundaries with deterministic small-region merging and
  FastCDC sub-chunking of oversized regions. Acceptance: two synthetic RL
  checkpoints with 98% bit-identical tensors store <5% of file size as delta
  through the gateway.
- **Delta chains** (D-028): parent via `x-amz-meta-sparta-parent` or MinHash
  LSH detection over stored manifests (threshold `SPARTA_DELTA_MINHASH_
  THRESHOLD`); new chunks encode against the positionally aligned parent chunk
  as a raw zstd dictionary when it beats plain by >10%; lineage recorded in
  `manifest.parent`; dict refs flow through restore, GC mark, compaction
  remap, and rebuild-index.
- **TrailStore v1**: `train-dict --bucket --prefix` samples the corpus, trains
  a zstd dictionary (stored under `_sparta/dicts/` + durable rules map),
  applied to future writes under the prefix; `tier --bucket` applies
  `SPARTA_TIER_RULES` age rules as storage-class transitions on packs whose
  referencing manifests all match the rule prefix (D-030).
- **Meter**: multi-process-safe appends (flock + tail resync, D-029) — GC,
  train-dict, and tier log into the same chain as the serving gateway; found
  by an integration test catching a real chain break.

## M6 — Deployment and customer readiness (2026-08-28)

- **Dockerfile**: two-stage build, slim runtime image carrying all four
  binaries (gateway entrypoint), non-root user, `/data` volume for state.
- **docker-compose.dev.yml**: MinIO + gateway (active) + raw-corpus seeder +
  through-gateway seeder, health-gated, one command.
- **Helm chart**: full values surface (upstream, secret-ref credentials, mode,
  bypass ConfigMap file, GC/ingest/delta/tiering knobs, cache + state PVCs,
  resources), probes on /healthz + /readyz, Recreate strategy (single-writer
  metadata), ALB/NLB failover pattern documented.
- **Terraform (AWS)**: ECS Fargate + internal NLB + minimal bucket-scoped IAM
  into an existing VPC (D-032).
- **Docs**: QUICKSTART (Docker + native paths, both exercising the full
  scanner→activation journey), OPERATIONS (bypass/failover runbook, complete
  env table, GC/rebuild/meter procedures, failure-mode table, monitoring),
  SECURITY (placement, encryption inheritance, nothing-leaves-the-VPC,
  integrity properties), SRF-SPEC (publishable measurement standard);
  top-level README.
- **Final acceptance**: `tools/quickstart-native.sh` runs clone→deployment→
  SRF report end to end; measured 27 s warm on the dev machine, 188 s
  for a from-scratch release build — a fresh machine lands around 4 minutes
  total, far inside the 15-minute bar. Scanner-predicted SRF-S
  and engine-delivered SRF agree at 6.92x on the fixture fleet.
