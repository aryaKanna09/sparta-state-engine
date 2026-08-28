# Operating the Sparta State Engine

The prime directive (Invariant 0): **the engine must always be able to get out
of the way.** Every procedure here is written so that the worst outcome of any
engine failure is raw passthrough at raw-endpoint cost — never a failed
customer request, never data loss.

## 1. Modes

| Mode | What it does |
|---|---|
| `SPARTA_MODE=shadow` (default) | Pure passthrough. Streamed bytes are fingerprinted on the fly for a dedup-only savings estimate; nothing about stored data changes. Run this first. |
| `SPARTA_MODE=active` | Write-side dedup. New writes pass through raw, then a background worker ingests them into packs + manifests and deletes the raw. Reads are manifest-first, raw-passthrough otherwise. Objects that existed before activation are never touched. |

## 2. Bypass and failover

Three independent layers get the engine out of the way:

1. **Per-request bypass flag.** `SPARTA_BYPASS=1` (checked every request) or —
   because a running process cannot see env edits — create the file named by
   `SPARTA_BYPASS_FILE`. In Kubernetes this is a mounted ConfigMap: add any
   key at the mount path to flip to dumb proxy, remove it to resume. No
   restart, effective on the next request.
2. **Internal fallback.** Every handler falls back to raw passthrough on any
   internal error (panics included). Engine-side observation (taps, metering,
   the local DB) is structurally unable to fail a request.
3. **Load-balancer failover.** `/healthz` is liveness (process up). `/readyz`
   returns 503 the moment the upstream is unreachable. Put the gateway behind
   an ALB/NLB with a second target group pointing at the raw S3 endpoint
   (interface VPC endpoint IPs) so LB health checks shift traffic to raw
   storage automatically. Clients keep working; savings pause.

**Emergency runbook** (suspected engine misbehavior):
`touch` the bypass file (or set `SPARTA_BYPASS=1` and restart) → traffic is
raw passthrough → investigate with `sparta-gateway stats`, the meter log, and
gateway logs. Note: in active mode, objects already ingested are served from
packs via manifest lookups even under bypass? **No** — bypass skips the
engine entirely, so ingested objects (whose raw was deleted) will 404 through
a bypassed gateway. For engine-written data the correct degraded path is the
internal fallback + LB failover, not the bypass flag; use bypass when you
still have raw objects (shadow mode, or active before much ingestion) or
during a controlled migration. This is the one sharp edge of active mode:
plan LB failover for reads *before* activating.

## 3. Configuration reference

All configuration is environment variables.

| Variable | Default | Meaning |
|---|---|---|
| `SPARTA_LISTEN` | `0.0.0.0:8014` | Listen address |
| `SPARTA_UPSTREAM_ENDPOINT` | (required) | Customer's real S3 endpoint URL |
| `SPARTA_UPSTREAM_REGION` | `us-east-1` | Upstream region |
| `SPARTA_ACCESS_KEY` / `SPARTA_SECRET_KEY` | unset | Credentials clients sign with; unset = unauthenticated dev mode |
| `SPARTA_UPSTREAM_ACCESS_KEY` / `SPARTA_UPSTREAM_SECRET_KEY` | = gateway keys | Separate upstream credentials (else SDK default chain / task role) |
| `SPARTA_MODE` | `shadow` | `shadow` or `active` |
| `SPARTA_BYPASS` | unset | `1` = dumb proxy, evaluated per request |
| `SPARTA_BYPASS_FILE` | unset | Path whose existence = bypass (restart-free toggle) |
| `SPARTA_DB_PATH` | `sparta-gateway.sqlite` | Local metadata cache (rebuildable) |
| `SPARTA_METER_PATH` | `sparta-meter.jsonl` | Tamper-evident meter log |
| `SPARTA_METER_LEVEL_INTERVAL_S` | `3600` | Storage level gauge cadence |
| `SPARTA_FORCE_PATH_STYLE` | `1` | Path-style addressing upstream |
| `SPARTA_SHADOW_CHUNKING` | `1` | Fingerprint streams in shadow mode |
| `SPARTA_UPSTREAM_MAX_RETRIES` | `3` | Upstream retry budget |
| `SPARTA_UPSTREAM_ATTEMPT_TIMEOUT_MS` | unset | Per-attempt upstream timeout |
| `SPARTA_INGEST_BATCH` | `64` | Objects per ingest batch |
| `SPARTA_INGEST_MAX_BYTES` | `2147483648` | Objects above this stay raw forever |
| `SPARTA_MAX_NORMALIZE_BYTES` | `1073741824` | Decompression bound for normalization |
| `SPARTA_RESTORE_CONCURRENCY` | `8` | Parallel coalesced fetches per GET |
| `SPARTA_COALESCE_GAP_BYTES` | `1048576` | Max gap merged into one ranged GET |
| `SPARTA_CACHE_DIR` | unset | NVMe chunk cache directory (unset = no cache) |
| `SPARTA_CACHE_BYTES` | `10737418240` | Chunk cache capacity |
| `SPARTA_PIN_PREFIXES` | unset | Comma-separated key prefixes whose chunks are pinned in cache |
| `SPARTA_GC_GRACE_DAYS` | `7` | Sweep grace period for unreferenced packs |
| `SPARTA_GC_COMPACT_THRESHOLD` | `0.3` | Garbage fraction that triggers pack compaction (≥1 disables) |
| `SPARTA_DELTA` | `1` | Parent-delta encoding on new chunks |
| `SPARTA_DELTA_MINHASH_THRESHOLD` | `0.7` | Jaccard threshold for automatic parent detection |
| `SPARTA_TIER_RULES` | unset | `prefix=days:CLASS,...` age tiering rules |
| `RUST_LOG` | `info` | Log filter |

## 4. Routine operations

### Metering and reporting
```bash
sparta-meter --log /state/sparta-meter.jsonl verify           # chain intact?
sparta-meter --log /state/sparta-meter.jsonl report           # trailing-30d net savings + VSRR
sparta-meter --log /state/sparta-meter.jsonl report --cur cur-export.csv.gz   # with your real rates
sparta-meter --log /state/sparta-meter.jsonl invoice --month 2026-08
sparta-meter --log /state/sparta-meter.jsonl export --format parquet --out meter.parquet
```
The meter log is multi-writer-safe (the gateway and CLI runs share one chain).
`verify` failing means the file was edited or truncated mid-file — treat as an
incident; the chain pinpoints the first bad record.

### Garbage collection
```bash
sparta-gateway gc --bucket <bucket> --dry-run   # full report, zero writes
sparta-gateway gc --bucket <bucket>             # compaction + mark
sparta-gateway gc --bucket <bucket>             # (later) sweep after grace
```
GC is three-phase: chunk-refcounted mark from all manifests → compaction of
packs whose unreferenced fraction ≥ threshold (live chunks rewritten to new
packs, manifests updated) → two-phase sweep (bucket markers, deletion only
after `SPARTA_GC_GRACE_DAYS`). Run it on a schedule (daily CronJob is
typical); every sweep, dry runs included, is logged to the meter. Safe to
interrupt anywhere.

### Rebuilding the local index
```bash
sparta-gateway rebuild-index --bucket <bucket> [--bucket <bucket2>...]
```
The SQLite DB is a cache: manifests in the bucket embed everything needed to
reconstruct it. Reads work even with a cold DB (manifest fallback); rebuild
restores listings and dedup indexes. Run after losing/replacing the state
volume. Corrupt DB symptoms (SQLITE errors in logs) → stop, delete the DB
files, rebuild-index, start.

### Dictionary training and tiering (ratio features)
```bash
sparta-gateway train-dict --bucket <bucket> --prefix trajectories/
SPARTA_TIER_RULES="snapshots/=30:GLACIER_IR" sparta-gateway tier --bucket <bucket> [--dry-run]
```
Training needs ≥4 ingested objects under the prefix; the dictionary applies to
future writes there. Tiering transitions a pack only when *every* referencing
object matches the rule prefix and the oldest referencing manifest exceeds the
age — mixed packs never move.

## 5. Failure modes

| Symptom | Cause | Behavior | Action |
|---|---|---|---|
| Requests succeed, no savings growth | Bypass active, or ingest worker erroring | Raw passthrough; raws accumulate | Check bypass file/env; grep logs for `ingest batch failed` |
| `/readyz` 503 | Upstream unreachable | LB fails over to raw endpoint | Fix upstream networking; gateway rejoins automatically |
| `CRITICAL: restored object hash mismatch` | Pack/manifest corruption | Manifest quarantined; raw fallback if present; that object 404s if raw is gone | Incident: keep the quarantined manifest, inspect the pack (`_sparta/packs/`), restore from replication/backup |
| SQLite errors in logs | Damaged state volume | Reads continue via bucket manifests; listings degrade to raw-only | rebuild-index onto a fresh volume |
| `meter chain broken` | Edited/truncated meter log | Reports refuse the log | Restore the log from the bucket mirror/backup; investigate |
| Objects > ingest bound | By design | Stay raw passthrough forever | Raise `SPARTA_INGEST_MAX_BYTES` if desired |
| PUT latency grew after activation | It should not (writes are passthrough) | — | Check upstream latency itself; gateway adds ~ms |

## 6. What to monitor

- `/healthz`, `/readyz` (alert on readiness flapping).
- `sparta-gateway stats` counters: op counts, `bytes_in`/`bytes_out`.
- Meter `report`: `net_savings_usd` trend, `contains_projections=false` once
  active, `gc_sweeps` advancing.
- Ingest queue drain: `ingest_queue` rows should be transient; sustained
  growth = worker failing (logs) or undersized CPU.
- Bucket-side: `_sparta/packs/` byte growth vs logical write volume (the live
  SRF), and leftover raw objects older than a few minutes in active mode.
- Log lines at `WARN` and above; anything `CRITICAL` pages.

## 7. Upgrades and restarts

The gateway is stateless beyond the local cache volume; kill/restart is safe
at any point (write-through means a crash strands at most some un-ingested
raws, which the worker picks up on the next start). Use `Recreate` strategy /
`desired_count=1`: the metadata DB assumes a single writer.
