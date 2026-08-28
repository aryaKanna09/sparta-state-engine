# Sparta State Engine

An S3-compatible deduplicating storage gateway for AI agent infrastructure.

Agent fleets generate massive redundant state — sandbox snapshots, forked
environments, model checkpoints, trajectory data. The engine sits between your
applications and **your own object storage**, eliminates redundancy with
content-defined chunking, compression normalization, and delta encoding, and
meters verified savings. Everything runs inside your environment; your data
never leaves your bucket.

```
your apps ──S3 API──▶ sparta-gateway ──S3 API──▶ your bucket
                        │                          ├── your objects (pre-existing, untouched)
                        │  chunk / dedup /         └── _sparta/
                        │  compress / verify            ├── packs/       deduped chunk data
                        │                               ├── manifests/   object metadata + BLAKE3
                        └── meter (tamper-evident)      └── dicts/, gc/  ratio + lifecycle state
```

## Start here

- **[docs/QUICKSTART.md](docs/QUICKSTART.md)** — clone → running local
  deployment with an SRF report in minutes (Docker or native).
- **[docs/OPERATIONS.md](docs/OPERATIONS.md)** — modes, bypass and failover,
  full env-var reference, GC, rebuild, monitoring, failure modes.
- **[docs/SECURITY.md](docs/SECURITY.md)** — data placement, encryption
  inheritance, what leaves your VPC (nothing).
- **[docs/SRF-SPEC.md](docs/SRF-SPEC.md)** — the State Redundancy Factor
  measurement spec (published parameters, reproducible across
  implementations).

## The invariant that outranks everything

**The engine must always be able to get out of the way.** Every request
handler falls back to raw passthrough on any internal error; `SPARTA_BYPASS`
(env or file) turns the gateway into a dumb proxy per request with no restart;
`/healthz` and `/readyz` drive load-balancer failover to the raw endpoint. A
fault-injection suite (dead metadata DB, panicking chunker, corrupt packs,
upstream 500s/timeouts) gates every merge.

## Workspace

| Crate / dir | What it is |
|---|---|
| `crates/sparta-core` | Chunking (FastCDC + checkpoint-aware), BLAKE3 addressing, verified compression normalization, packfile + manifest formats, MinHash — no I/O |
| `crates/sparta-scanner` | Shadow A: read-only SRF-S assessment of any bucket, JSON + single-file HTML reports |
| `crates/sparta-gateway` | The gateway: shadow + active modes, write-through ingest, verified restore, GC with compaction, rebuild-index, dictionary training, tiering |
| `crates/sparta-meter` | Append-only hash-chained metering, pricing (AWS rate card + CUR), NET savings / VSRR, invoices, CSV/Parquet export |
| `crates/sparta-testkit` | Fixture generators (agent fleet, RL checkpoints), MinIO harness, fault-injection proxy, client workflow scripts |
| `deploy/` | Dockerfile, docker-compose dev stack, Helm chart, Terraform (AWS/ECS) |

## Development

```bash
cargo test --workspace                      # unit + property tests
cargo test --workspace -- --ignored        # integration (needs tools/bin/minio)
bash tools/quickstart-native.sh            # full local stack + SRF report
```

CI runs fmt, clippy (`-D warnings`), all tests, the fault-injection suite, and
the fleet SRF benchmark. `DECISIONS.md` records every judgment call;
`SPEC.md` is the build spec of record.
