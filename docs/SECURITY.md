# Security Model

## The one-sentence version

The engine runs inside your environment, stores everything it produces in
your own bucket under `_sparta/`, inherits your bucket's encryption, and by
default sends **nothing** anywhere else.

## Data placement

| Artifact | Where | Contains |
|---|---|---|
| Packfiles (`_sparta/packs/*.pack`) | Your bucket | Your object data, chunked and compressed |
| Manifests (`_sparta/manifests/*.json`) | Your bucket | Object metadata: key, sizes, BLAKE3 digests, chunk locations, content type, user metadata, lineage |
| GC markers (`_sparta/gc/candidates/`) | Your bucket | Pack ids + timestamps |
| Dictionaries (`_sparta/dicts/`) | Your bucket | zstd dictionaries trained on your data |
| Meter log (`sparta-meter.jsonl`) | Gateway state volume | Byte/request/compute counters, object **counts and sizes only** — no keys beyond bucket names, no content |
| Metadata DB (`*.sqlite`) | Gateway state volume | Cache of the above; rebuildable from the bucket, losable without data loss |
| Chunk cache (`SPARTA_CACHE_DIR`) | Gateway NVMe | Decompressed chunks of recently read objects |

Everything needed to reconstruct any object exists in the bucket itself; the
gateway's local volumes are caches and accelerators.

## Encryption

The engine writes packs, manifests, and all other artifacts with plain
`PutObject` calls, so they inherit the bucket's **default encryption**
configuration (SSE-S3 or SSE-KMS with your bucket key) exactly like any other
object your applications write. The engine never sets, overrides, or needs to
know encryption parameters, and never handles KMS material. Objects protected
by SSE-C (client-provided keys) cannot be read by the engine and pass through
untouched.

In transit: gateway↔upstream uses whatever scheme the endpoint URL specifies —
use the HTTPS regional endpoint or an interface VPC endpoint. Client↔gateway
runs inside your network; terminate TLS at your load balancer or mesh
according to your own policy.

## Credentials and authentication

- Clients authenticate to the gateway with AWS SigV4 (header and presigned
  URL forms) against `SPARTA_ACCESS_KEY`/`SPARTA_SECRET_KEY`. Unset means
  unauthenticated — dev mode only, never production.
- Upstream, the gateway uses either the same keys (credential passthrough),
  explicitly provided upstream keys, or — recommended on AWS — no static keys
  at all: the SDK default chain picks up the ECS task role / IRSA role scoped
  to the one bucket (see `deploy/terraform` for the minimal policy).
- There are no user accounts, no roles, no auth database in the engine. Auth
  beyond S3 credential passthrough is an explicit non-goal.

## What leaves your VPC

**Nothing, by default.** The gateway opens connections only to
`SPARTA_UPSTREAM_ENDPOINT`. There is no telemetry, no phone-home, no update
check, no license ping in the binary. Billing works from the meter log you
hold: you (or Sparta, with access you grant) run `sparta-meter report`
against it. Sharing metering **totals** (dollar/byte aggregates, never keys or
content) with Sparta is opt-in and something you do out-of-band — the current
build contains no mechanism to transmit them.

## Trust and integrity properties

- **Byte identity**: every full GET in active mode recomputes the object's
  BLAKE3 and compares it to the manifest before the final byte is released;
  chunk reads always verify each chunk's own digest. A mismatch quarantines
  the manifest (CRITICAL log) and falls back to the raw object if it exists.
- **Tamper-evident metering**: each meter record is BLAKE3-chained over its
  predecessor; edits, insertions, and deletions anywhere in history are
  detected by `sparta-meter verify`. (Truncation of the newest tail is
  detectable only against an externally mirrored chain tip — mirror the log
  if your billing dispute model needs it.)
- **Fail-open, in the safe direction**: engine errors degrade to raw
  passthrough of customer traffic, never to serving wrong bytes. The fault
  injection suite in CI proves passthrough under dead metadata DB, panicking
  chunker, corrupt packs, and upstream failures.

## Attack surface notes

- The gateway exposes exactly one protocol surface (S3 API) plus `/healthz`
  and `/readyz`. Keep it on private networks; it has no reason to be
  internet-reachable.
- Object keys and user metadata appear in manifests inside your bucket and in
  gateway logs at debug level — treat log sinks with the same sensitivity as
  bucket listings.
- The engine parses untrusted object content (gzip/zstd headers, safetensors
  JSON, zip directories). All parsers are bounds-checked, allocation-capped
  (`SPARTA_MAX_NORMALIZE_BYTES` guards decompression bombs), and a parse
  failure of any kind degrades to opaque handling of that object.
- Denial-of-service posture: per-request memory on the write path is bounded
  by streaming (multipart parts, tap buffers ≤ a few chunk sizes); ingest
  materializes at most `SPARTA_INGEST_MAX_BYTES` per object, one object at a
  time.

## Reporting

Report suspected vulnerabilities to security@usesparta.co. Please do not file
public issues for security reports.
