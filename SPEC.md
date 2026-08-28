# CLAUDE CODE BUILD PROMPT: Sparta State Engine MVP

Copy everything below this line into Claude Code as the opening prompt. Keep this file in the repo root as SPEC.md so every session can reference it.

## Mission

You are building the MVP of the Sparta State Engine: an S3 compatible deduplicating storage gateway for AI agent infrastructure. Agent fleets generate massive redundant state (sandbox snapshots, forked environments, model checkpoints, trajectory data). The engine sits between the customer's applications and their own object storage, eliminates redundancy via content defined chunking and delta encoding, and meters verified savings. Everything runs inside the customer's environment. Their data never leaves their bucket.

Build the complete MVP described below, milestone by milestone, in order. This is intended to be as close to customer deployable as possible: real error handling, real tests, real docs, no placeholder logic in critical paths. Where a genuine product decision is unresolved, implement the stated default and record the alternative in DECISIONS.md rather than stopping.

## Non goals, absolute

Do not build any of the following, regardless of how natural an extension seems: host level integration of any kind (no CRIU, no memory state capture, no agent daemons on customer hosts), semantic or embedding based dedup, rewriting or migrating historical objects already in the bucket, any protocol surface other than the S3 API, multi node or distributed metadata, a SaaS control plane, or user accounts and auth beyond S3 credential passthrough. If a task seems to require one of these, write the smallest stub interface and move on.

## Tech stack

Rust, stable toolchain. S3 protocol surface via the `s3s` crate (do not hand write the protocol layer; unsupported S3 operations must transparently proxy to the upstream endpoint rather than erroring). Chunking via the `fastcdc` crate. Hashing via the official `blake3` crate. Compression via `zstd` and `flate2`. Embedded metadata via SQLite through `rusqlite` (WAL mode). HTTP client via `aws-sdk-s3` for upstream calls. CLI via `clap`. Structured logging via `tracing`. If any crate choice proves unworkable, choose the nearest maintained equivalent and log the substitution in DECISIONS.md.

## Repository layout

Create a cargo workspace:

* `crates/sparta-core`: chunking, hashing, compression normalization, packfile format, manifest format, shared types. No I/O beyond traits.
* `crates/sparta-scanner`: Shadow A standalone binary.
* `crates/sparta-gateway`: the S3 gateway binary (Shadow B and active mode).
* `crates/sparta-meter`: append only metering log, pricing, report generation.
* `crates/sparta-testkit`: fault injection harness, fixture generators, localstack/minio integration helpers.
* `deploy/`: Dockerfile (single static-ish binary image), docker-compose.dev.yml (gateway + MinIO + seeded fixtures), Helm chart, Terraform module (AWS first).
* `docs/`: SRF-SPEC.md, OPERATIONS.md, SECURITY.md, QUICKSTART.md.

## Fixed constants (the SRF spec, do not change without editing SRF-SPEC.md)

FastCDC min/avg/max chunk size 256 KiB / 1 MiB / 4 MiB. BLAKE3, 256 bit digests. zstd level 3 for pack compression. Compression normalization ON by default. These are published spec parameters; reproducibility of the SRF number across implementations matters more than tuning.

## Invariant 0, the prime directive

The engine must always be able to get out of the way. This is the most important requirement in the entire build:

1. Every request handler in the gateway wraps all dedup logic in a fallback: on ANY internal error (panic caught at the handler boundary, corrupt index, timeout, unsupported operation), the request is proxied byte for byte to the customer's real upstream endpoint and a structured error is logged. The customer's workload must never fail because Sparta failed.
2. Environment flag `SPARTA_BYPASS=1` turns the entire gateway into a dumb proxy instantly, evaluated per request, no restart required.
3. Health endpoint `/healthz` (liveness) and `/readyz` (readiness) suitable for load balancer failover to the raw endpoint.
4. The testkit must include a fault injection suite that proves all of this: kill the metadata DB mid request, corrupt a packfile, inject upstream 500s and timeouts, force panics in the chunker, and assert that client operations still succeed via passthrough with correct bytes. This suite is a merge gate for every milestone after M2.

## Data placement and durability rules

All engine artifacts (packfiles, manifests, indexes, meter logs) live in the customer's own bucket under the `_sparta/` prefix, using the customer's encryption settings. The embedded SQLite database is a cache and accelerator only: every piece of metadata needed to reconstruct any object must also exist as objects under `_sparta/`, so a lost metadata volume means a slow rebuild, never data loss. Implement `sparta-gateway rebuild-index` which reconstructs the full local DB from the bucket alone, and test it.

## Milestones, in build order

### M0: Core library

`sparta-core` with: FastCDC chunking at spec parameters; BLAKE3 addressing; compression detection by magic bytes (gzip, zstd, xz, zip); compression normalization (decompress, chunk raw bytes, record a recompression recipe with algorithm, level, and framing metadata sufficient to reproduce the original compressed bytes exactly); recompression verification (recompress and compare BLAKE3 digests, and on any mismatch mark the object opaque so it is stored and returned as original bytes with no dedup); packfile format (chunks appended into 32 to 64 MB packs, written in manifest order, with a per pack index of chunk hash to offset/length); manifest format (an object is an ordered list of chunk references plus original metadata, content type, recompression recipe, optional parent pointer); MinHash similarity sketch over chunk hashes for parent detection. Unit tests for every component, including a property test: for arbitrary inputs, chunk, pack, reconstruct, and assert byte identity, including through the compression normalization path.

### M1: Scanner (Shadow A)

`sparta-scanner`, a standalone read only binary. Input: S3 credentials, bucket list, optional S3 Inventory manifest path, sampling rate. Behavior: enumerate objects (Inventory preferred, ListObjectsV2 fallback), sample with ranged GETs under a configurable byte budget, fingerprint with the spec chunker including compression normalization, estimate fleet wide redundancy from the sample. Output: SRF-S point estimate with a confidence interval derived from sampling variance, a breakdown by prefix and content type, top redundancy clusters, and an addressable SRF that excludes objects matched by user supplied exclusion rules (tags or prefix patterns for DR and replication copies). Report emitted as both JSON and a clean single file HTML report suitable for sending to a customer. Flag prominently in the report when compression normalization changed the estimate by more than 3x, marking the result as requiring reverification. The scanner must run to completion on a 100k object bucket in under an hour at 1 percent sampling on developer hardware against MinIO.

### M2: Gateway passthrough and Shadow B

`sparta-gateway` serving the S3 API via `s3s`: PUT, GET, HEAD, DELETE, multipart upload (create, upload part, complete, abort), ListObjectsV2, CopyObject, and presigned URL passthrough. In shadow mode (the default on first boot), every operation proxies to the upstream endpoint untouched while the engine fingerprints streamed bytes on the fly and records logical byte counts. Implement Invariant 0 fully here, plus the fault injection suite in `sparta-testkit`, and wire it into CI. Acceptance: an unmodified `aws s3` CLI and `boto3` test script (include it in testkit) pass a full workflow (multipart upload of a 6 GB file, list, ranged get, copy, delete) through the gateway in shadow mode against MinIO with byte identical results, with and without every fault injected.

### M3: Meter

`sparta-meter`: an append only, tamper evident log (each record carries a hash chain over the previous record) of logical versus physical bytes per operation, request counts by type, and engine compute time. Pricing layer: a built in rate card (AWS S3 standard tiers, request prices, egress) plus a loader for AWS CUR exports to use the customer's real rates when provided. Savings are computed NET: subtract the request costs the pack layer spends and an estimate of engine compute. Outputs: VSRR for the deployment (trailing 30 day metered savings, annualized), exportable as CSV and Parquet, plus a monthly invoice draft derived only from the meter log. Shadow B baseline and active mode consumption must both come from this same log so the before and after comparison is internally consistent. Include `sparta-meter verify` which revalidates the hash chain.

### M4: Activation, the write side dedup path

Active mode, write side only: newly written objects flow through ingest (sniff, normalize, chunk, dedup lookup, pack, manifest); existing objects are untouched and read as raw passthrough forever unless they were written through the engine. Restore path: manifest to pack ranges, parallel ranged GETs with adjacent range coalescing, streaming reconstruction in key order, and an NVMe backed LRU chunk cache with pinning support for base template chunks. Garbage collection: reference counted chunks, mark and sweep with a 7 day epoch grace period (config: `SPARTA_GC_GRACE_DAYS`), a `--dry-run` mode that reports exactly what would be deleted, and every sweep logged to the meter log. Acceptance: the boto3 workflow passes in active mode with byte identity verified via BLAKE3 on every GET against the original PUT bytes; a simulated fleet fixture (testkit generates 200 sandbox snapshots derived from 3 base templates with realistic mutation rates, both raw and gzipped variants) achieves a measured SRF-S of at least 5 through the engine; rebuild-index after deleting the local DB restores full read capability; GC dry run then real run removes only unreferenced chunks and the fault suite still passes.

### M5: Ratio work

Checkpoint aware chunking: detect safetensors (parse the JSON header for per tensor offsets) and PyTorch zip checkpoints (per entry boundaries), and chunk at tensor boundaries; testkit generates two adjacent synthetic RL checkpoints with 98 percent bit identical tensors and the measured delta stored must be under 5 percent of file size. Delta chains: when a parent is declared via the `x-amz-meta-sparta-parent` header or detected via MinHash similarity above a threshold, delta encode new chunks against the parent region using zstd with a parent derived dictionary, and record lineage in the manifest graph. TrailStore v1 as policy inside the gateway: age based tiering rules (configurable per prefix) executed via storage class transitions on the packs the engine owns, and per corpus zstd dictionary training (`sparta-gateway train-dict --prefix`) applied to future writes under that prefix.

### M6: Deployment and customer readiness

Dockerfile producing a minimal image; docker-compose.dev.yml bringing up gateway, MinIO, and seeded fixtures in one command; Helm chart with values for upstream endpoint, credentials via secret ref, cache volume, bypass flag, GC grace, and resource limits, including a documented ALB/NLB failover pattern to the raw endpoint; Terraform module for AWS deployment into an existing VPC. Docs: QUICKSTART.md (the install one chart, change one URL walkthrough, tested end to end), OPERATIONS.md (bypass procedure, rebuild-index, GC, failure modes, what the customer should monitor), SECURITY.md (data placement, encryption inheritance, exactly what leaves the VPC: nothing by default, metering totals opt in), SRF-SPEC.md (the full measurement spec with the fixed constants, written to be publishable as an open standard). Final acceptance: a fresh machine goes from git clone to a working local deployment with a generated SRF report on the fixture fleet in under 15 minutes following QUICKSTART.md alone.

## Cross cutting requirements

Every GET in active mode verifies the reconstructed object's BLAKE3 against the manifest before returning the final byte; on mismatch, serve from upstream raw if available, log CRITICAL, and quarantine the manifest. All configuration via environment variables with a documented table. CI (GitHub Actions): fmt, clippy with deny warnings, unit tests, integration tests against MinIO, the fault injection suite, and the fixture fleet SRF benchmark with the ratio asserted. Performance sanity targets, not hard gates: passthrough overhead under 5ms p50 added latency on 1 MB objects locally; ingest at 200 MB/s or better per core on the fixture data. Commit at every green milestone with a summary in CHANGELOG.md. Maintain DECISIONS.md for every judgment call made.

## Working style

Work strictly in milestone order; do not start a milestone until the previous one's acceptance criteria pass in CI. If something in this spec is ambiguous, take the interpretation that best serves Invariant 0 and byte identity, implement it, and note it in DECISIONS.md. Ask nothing that a reasonable senior engineer would decide alone. Bias every tradeoff toward correctness over ratio: a smaller savings number that is true beats a larger one that is not, because the meter is the product.
