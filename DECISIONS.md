# DECISIONS.md

Judgment calls made during the build, in chronological order. Each entry records the
decision, the default implemented, and the alternative considered.

## D-001: Repo location and layout
The build environment's working directory (`~/Desktop`) contained no existing Rust repo,
so the workspace was created fresh at `sparta-state-engine/`. SPEC.md kept at repo root
per the build prompt.

## D-002: No Docker on the dev machine — native MinIO for integration tests
The development machine has no Docker daemon. Integration tests and the local acceptance
runs use a native MinIO server binary (downloaded from the official dl.min.io release
channel into `tools/bin/`, gitignored) driven by `sparta-testkit`. The
`deploy/docker-compose.dev.yml` path is still authored and is the documented QUICKSTART
path for machines with Docker; CI (GitHub Actions, Linux) runs MinIO natively as well so
the same harness covers both. Alternative: require Docker for all dev — rejected because
it would make the build unverifiable on this machine.

## D-003: Compression normalization scope for MVP
Detection by magic bytes covers gzip, zstd, xz, and zip as specified. Full normalization
(decompress → chunk raw → recompression recipe → byte-exact verification) is implemented
for **gzip** and **zstd**, the two formats with recompression parameters we can capture
faithfully with the mandated crates (`flate2`, `zstd`). **xz** and **zip** are detected
and recorded in the manifest as `detected_compression`, but stored **opaque** (original
bytes, chunked as-is) in the MVP: xz would add a C dependency (`liblzma`) and zip requires
per-entry recipes; both recompression paths have poor byte-exactness odds, and the spec's
mismatch rule would mark most of them opaque anyway. This biases toward byte identity per
the working style. Alternative: attempt xz/zip normalization and rely on the verify-or-
opaque rule — deferred, the hook (`CompressionKind::{Xz, Zip}`) is in place.

Note that opaque objects still get CDC chunking and dedup on their raw (compressed)
bytes; "opaque" only means the normalization path is skipped.

## D-004: gzip recipe fidelity
`flate2` (miniz_oxide backend) does not reproduce arbitrary gzip streams produced by
other encoders (zlib levels/strategies, header fields, multi-member streams). The recipe
records compression level (mapped from the gzip XFL byte), mtime, OS byte, FNAME/FCOMMENT
fields, and the exact trailer. Verification recompresses with `flate2` at the recorded
level and compares BLAKE3 of the full stream; on any mismatch the object is stored opaque
per spec. Practically: gzip streams produced by common tools at default settings often
round-trip; those that don't are safely opaque. No attempt is made to bit-match foreign
deflate implementations.

## D-005: zstd recipe fidelity
The zstd frame header does not record the compression level, so a recipe can only be
verified by trial: we try the spec level (3) first, then the other standard levels 1..=19
in a fixed probe order, and record the first level whose recompression BLAKE3-matches.
Objects compressed with non-default window/params or dictionaries won't match and go
opaque. Probe cost is bounded by input size and only paid once at ingest.

## D-006: MinHash parameters
128 permutations (banded 32×4 for clustering in the scanner), over the set of chunk
BLAKE3 hashes, permutation = 64-bit xxhash-style mix keyed by permutation index applied to
the first 8 bytes of each chunk hash. 128×8B = 1 KiB per object sketch. Fixed in
SRF-SPEC.md. Alternative: b-bit minwise hashing for smaller sketches — not worth the
complexity at MVP scale.

## D-007: Packfile compression granularity
Chunks are individually zstd-compressed (level 3) inside packs, so any chunk can be
fetched with a single ranged GET without decompressing neighbors. The per-pack index maps
chunk hash → (offset, compressed_len, raw_len, codec). Alternative: whole-pack
compression frames for better ratios on small chunks — rejected: it couples restore
latency to pack layout, and the per-chunk framing keeps the restore path trivially
parallel. Dictionary compression (M5) recovers most of the lost ratio.

## D-008: Edition 2021
Workspace pins edition 2021 (not 2024) for maximum compatibility with the mandated crate
set on stable; nothing in the codebase needs 2024 features.

## D-009: Jackknife CI instead of bootstrap for SRF-S
The naive bootstrap is structurally biased for dedup ratios: resampling with
replacement duplicates objects, and a duplicated object dedups perfectly
against itself, inflating every resampled ratio above the point estimate (first
implementation produced CIs that did not contain their own point estimate).
Replaced with an exact leave-one-out jackknife (95% = point ± 1.96·SE), which
is centered by construction and computable in O(total chunks) using global
chunk occurrence counts. `ci_method` is recorded in the report JSON.

## D-010: Localized (hotspot) mutation model in the fleet fixture
Scattering the mutation budget uniformly across a snapshot dirties virtually
every 1 MiB-average chunk and models nothing real; sandbox writes cluster in a
few files. The fixture spends the budget in ~256 KiB contiguous hotspots plus
an appended log tail. Sibling snapshots therefore share family/template chunks
(the redundancy the engine actually recovers) rather than pairwise-identical
noise.

## D-011: Scanner sampling policy
Uniform seeded object sampling (partial Fisher–Yates, `--seed`, default 42),
byte budget reserved before each fetch (unbiased truncation because sample
order is random), and a per-object fetch cap: objects above `--max-object-bytes`
are fingerprinted on a prefix window and skip normalization (a partial
compressed stream cannot be verified). SRF-S is reported as measured within
the sample and documented as a conservative lower bound on fleet SRF; tag
exclusion rules are evaluated on sampled objects only (GetObjectTagging per
object). S3 Inventory support is CSV format (the S3 default); ORC/Parquet
inventories are out of MVP scope.

## D-012: Bypass file in addition to the env flag
`SPARTA_BYPASS=1` is honored, but a running process cannot observe external
edits to its environment, so the env flag alone cannot deliver "no restart
required" on any real platform. Added `SPARTA_BYPASS_FILE`: when set, the
existence of that file (touch/rm, or a ConfigMap mount) toggles dumb-proxy
mode per request with zero restart. Both paths are covered by the fault suite.

## D-013: Shadow-mode savings estimate is dedup-only
Shadow taps forward bytes as they arrive and record chunk hashes + lengths;
compressing every chunk inline would sit zstd directly in the customer's write
path for a mode whose contract is "untouched passthrough". Shadow SRF is
therefore reported as a dedup-only lower bound (stored_est = raw_len, no
compression credit) and labeled as such; the scanner (Shadow A) and active
mode (M4) provide the compressed view. Conservative by design: the meter must
never report savings shadow mode didn't prove.

## D-014: Shadow taps do not normalize compressed uploads
Normalization requires the complete stream plus decompress-recompress-verify;
doing that inline in shadow mode would buffer whole objects. Shadow chunking
runs on the raw streamed bytes (compression detected and noted). Effect: the
shadow estimate undercounts redundancy hidden behind differing compression —
again a conservative lower bound.

## D-015: Fault-suite hooks compiled in, inert without env vars
`SPARTA_FAULT_PANIC_TAP=1` panics inside the tap on first feed;
`SPARTA_FAULT_DB_FAIL=1` fails every metadata-DB entry point. Both are checked
per request/stream and drive the real error-containment paths (not mocks);
they are no-ops unless explicitly set. Chosen over conditional compilation so
the shipped binary's fault handling is exactly what the suite proved.

## D-016: Upstream fault semantics
Transient upstream faults (500s, connection drops, slow first attempts) are
absorbed by the gateway's SDK retry layer (`SPARTA_UPSTREAM_MAX_RETRIES`,
default 3 retries; optional `SPARTA_UPSTREAM_ATTEMPT_TIMEOUT_MS`). A hard
upstream outage is forwarded to the client — behavior parity with the raw
endpoint (the gateway must never fail requests the raw endpoint would serve,
and cannot serve requests the raw endpoint couldn't). `/readyz` flips to 503
on upstream unreachability so load balancers fail over to the raw endpoint.

## D-017: Delegation surface generated as literal code
`#[async_trait]` expands before `macro_rules!` inside an impl, so macro-based
method delegation cannot compile. The 104 passthrough methods are generated as
literal code (script in the commit history); every S3 operation the `s3s-aws`
proxy supports — including ones the engine will never optimize — delegates to
the upstream with op counting and a panic net, satisfying "unsupported
operations must transparently proxy".

## D-018: Meter log granularity and placement
One meter record per client data operation (put/get/upload_part plus the
zero-byte data ops for request counts by type), single JSONL file, flushed per
append, fsynced on the level interval. Bucket-configuration operations are
counted in the local stats DB but not metered — they are not billable data
path. Segment rotation and `_sparta/meter/` mirroring land with the M4 bucket
write path (the meter API already exposes the chain tip for sealing).
Alternative: time-aggregated records — rejected for MVP; per-op records make
the invoice auditable line by line and volumes are modest at fleet scale.

## D-019: Storage savings need a time integral, not byte counters
Storage cost is $/GiB-month, so the report integrates Level gauge records
(step-hold) across the window instead of summing bytes written. Shadow mode
emits hourly projected levels (logical = bytes written through, physical =
dedup-only unique bound from the shadow chunk set) labeled `projected: true`;
reports and invoices that include projections say so explicitly. Active mode
(M4) emits metered actuals from manifest/pack accounting.

## D-020: VSRR definition
VSRR = net savings over the trailing 30 days, annualized (x12.1667). NET =
gross tiered-storage savings − engine upstream request costs (PUT/GET classes
priced separately) − engine compute (measured CPU time priced per vCPU-hour).
The chain-verified log is the only input; `sparta-meter verify` + `report`
reproduce every number.

## D-021: CUR loader scope
The CUR loader derives effective rates from stable lineItem columns only
(ProductCode/UsageType/UsageAmount/UnblendedCost), covering TimedStorage,
Requests-Tier1/Tier2, and DataTransfer-Out. Anything absent keeps the built-in
list price. Full CUR schema fidelity (RI/SP amortization, tiered rediscovery)
is out of MVP scope and noted in the rate card's `source` field.
