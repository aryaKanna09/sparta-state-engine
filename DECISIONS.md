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

## D-022: Active-mode writes are write-through with background ingest
A synchronous ingest path would either seal a tiny pack per PUT (violating the
32–64 MB pack spec and multiplying request overhead) or delay the client's ack
until a shared pack seals (unbounded latency, durability risk). Instead every
active-mode write (single PUT and multipart alike) passes through to the
upstream raw — the client's ack means exactly what raw S3 means — and an
ingest queue entry is recorded. A single background worker drains the queue in
order: GET raw → sniff/normalize/chunk/dedup → append to the shared pack
builder (sealed at 48 MB across objects, "written in manifest order") → upload
packs → write manifests → delete raws. Reads are manifest-first with raw
fallback, so every moment of the pipeline is consistent; a crash anywhere
leaves the raw in place (no loss, savings deferred). Per-key async locks
serialize ingest/delete/overwrite races (single-node gateway per the
non-goals). Cost: a transient raw-storage window and one extra GET+DELETE per
object, which the meter records honestly as engine overhead.

## D-023: Engine object layout under _sparta/
`_sparta/packs/<blake3-of-pack>.pack`, `_sparta/manifests/<blake3-of-key>.json`
(latest version only; the manifest embeds the real key, upstream ETag, content
type, and user metadata), `_sparta/gc/candidates/<pack>.json` two-phase GC
markers, `_sparta/meter/` mirrored meter log. Listings hide `_sparta/` from
clients and merge upstream raw objects with engine manifests (manifest wins on
key collision); delimiter grouping is applied over the merged stream with a
combined continuation token.

## D-024: GET verification granularity
Full-object GETs stream all chunks through a BLAKE3 hasher and hold back the
final chunk until the object hash matches the manifest; mismatch = CRITICAL
log, manifest quarantined, raw fallback if it exists. Ranged GETs verify each
served chunk's own BLAKE3 (decode_chunk always does) but cannot verify the
whole object without reading it; this is recorded as the spec-conformant
interpretation of "every GET verifies" for ranges.

## D-025: In-memory ingest with a size bound
The ingest worker materializes each raw object in memory (normalization needs
the full stream anyway, and chunk/pack work is slice-based). Objects above
`SPARTA_INGEST_MAX_BYTES` (default 2 GiB) stay raw passthrough forever: no
loss, no savings, logged at info. Disk-spooled streaming ingest for very large
objects is deliberate future work; for agent-fleet workloads (snapshots,
checkpoints in the hundreds of MB) the bound is rarely met.

## D-026: GC includes garbage-fraction compaction
Packs interleave chunks from many objects, so deleting a subset of objects
strands chunks *inside* still-referenced packs — pack-level sweeping alone
would never reclaim them, and the spec's "removes only unreferenced chunks"
would be hollow. GC therefore runs three passes per sweep: (1) mark builds
per-pack referenced-chunk sets from all manifests; (2) compaction rewrites any
pack whose unreferenced fraction ≥ `SPARTA_GC_COMPACT_THRESHOLD` (default
0.3) — live chunks are copied (already-encoded, no recompression) into fresh
packs, referencing manifests are rewritten, and the old pack becomes
unreferenced; (3) the two-phase sweep marks unreferenced packs with a bucket
candidate marker and deletes them once the marker outlives
`SPARTA_GC_GRACE_DAYS`. New packs before manifests before deletion makes every
crash window safe, and dry-run reports all three phases without writing.

## D-027: Merged-listing continuation tokens
The combined token is base64 of both sides' cursors (upstream `start-after`
string + engine DB cursor); skipping past an emitted CommonPrefix advances a
cursor to `prefix + U+10FFFF`, which sorts after every member key. This makes
pages restartable without server-side listing state, at the cost of one extra
upstream list call per page.

## D-028: Delta-chain encoding design
New (non-dedup) chunks are compressed three ways — plain zstd-3, against the
positionally aligned parent chunk as a raw-content dictionary, and against the
prefix's trained dictionary — and the smallest wins, with a 10% improvement
bar for dictionary codecs (each dict-coded read costs an extra chunk fetch).
The parent-dict ref is stored as a full nested `ChunkRef` in the manifest
(`dict`), so restore, GC marking, compaction remapping, and rebuild-index all
see it without consulting anything else; parent chunks must themselves be
plain, capping decode chains at one level. Pack-tail self-description cannot
carry the 32-byte parent hash (u64 dict_id field), so the pack-scan recovery
path cannot decode parent-delta chunks without manifests — recorded as an
accepted limitation (manifests are the durable metadata of record).

## D-029: Meter log is multi-writer by design
GC sweeps, train-dict, and tier runs are CLI processes that must log to the
same tamper-evident chain as the serving gateway. Appends are now serialized
with an advisory flock and each writer re-syncs the chain tail (reading only
the bytes appended since its last write) before sealing a new record. Caught
by the M5 integration tests as a real chain break; covered by a two-writer
unit test.

## D-030: Parent-detection threshold and tiering conservatism
`SPARTA_DELTA_MINHASH_THRESHOLD` defaults to 0.7 — deliberately conservative,
since chunk-set Jaccard between related snapshots of few-chunk objects is
structurally low, and a missed parent only costs compression opportunity while
a wrong parent records wrong lineage. Deployments with small objects should
lower it explicitly. Tiering transitions a pack only when every referencing
manifest matches the rule prefix and the oldest referencing manifest exceeds
the age — mixed packs never transition (correctness over ratio).

## D-031: QUICKSTART ships two equal paths
Path A is the specified docker-compose stack (gateway + MinIO + two seeders:
a raw corpus for the Shadow A scanner demo and a through-the-gateway corpus
for activation). Path B (`tools/quickstart-native.sh`) does the identical
journey with native binaries and the official MinIO binary — it exists because
this build machine has no Docker (D-002), and it doubles as the executable
form of the final acceptance test. Measured on this machine: 27 s with warm
binaries; the cold-build time is recorded in CHANGELOG. The native script is
also the honest demo of the product thesis: the scanner's predicted SRF-S and
the engine's delivered SRF match to the second decimal on the fixture fleet,
because both implement the same published measurement spec.

## D-032: Terraform module scope
ECS Fargate service + internal NLB + bucket-scoped task role into an existing
VPC, single task (single-writer metadata). The NLB health check targets
`/readyz` so upstream failure drains the target group; wiring the raw-endpoint
failover target group is documented (OPERATIONS §2) rather than templated —
it depends on whether the customer uses interface VPC endpoints, and shipping
a wrong default here would be worse than a documented pattern. The module is
authored and validated by review; it has not been applied against a live AWS
account from this environment.
