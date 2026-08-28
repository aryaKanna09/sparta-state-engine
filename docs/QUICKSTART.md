# Quickstart

From `git clone` to a running local deployment with an SRF report on the
simulated agent fleet. Two paths — Docker (one command) or native binaries
(no Docker required). Either fits in 15 minutes on a laptop.

## Path A — Docker

Requirements: Docker with Compose.

```bash
git clone <repo-url> sparta-state-engine
cd sparta-state-engine
docker compose -f deploy/docker-compose.dev.yml up --build -d
```

This starts MinIO (your stand-in "customer bucket"), the gateway in **active
mode** at `http://localhost:8014`, and a one-shot seeder that writes the
200-snapshot simulated agent fleet *through the gateway* so the engine ingests
it. Watch the seeder finish:

```bash
docker compose -f deploy/docker-compose.dev.yml logs -f seed-fixtures
```

Talk to your "bucket" through the engine with any unmodified S3 client:

```bash
export AWS_ACCESS_KEY_ID=sparta-dev AWS_SECRET_ACCESS_KEY=sparta-dev-secret AWS_REGION=us-east-1
aws --endpoint-url http://localhost:8014 s3 ls s3://fleet/snapshots/
aws --endpoint-url http://localhost:8014 s3 cp s3://fleet/snapshots/tpl0/sandbox-0000.tar.gz /tmp/x.tar.gz
```

Generate the SRF-S report. The stack also seeded a *raw* copy of the fleet
into a second bucket (`fleet-raw`) — that is what a customer bucket looks like
before Sparta, and what the read-only scanner assesses:

```bash
docker compose -f deploy/docker-compose.dev.yml exec gateway \
  sparta-scanner --bucket fleet-raw --endpoint http://minio:9000 \
  --sample-rate 1.0 --out /data/scan-report
docker compose -f deploy/docker-compose.dev.yml cp gateway:/data/scan-report/report.html ./report.html
open ./report.html
```

See the engine's own view of savings:

```bash
docker compose -f deploy/docker-compose.dev.yml exec gateway sparta-gateway stats
docker compose -f deploy/docker-compose.dev.yml exec gateway \
  sparta-meter --log /data/sparta-meter.jsonl report
```

## Path B — native binaries (no Docker)

Requirements: Rust stable toolchain, `curl`.

```bash
git clone <repo-url> sparta-state-engine
cd sparta-state-engine
bash tools/quickstart-native.sh
```

The script does, visibly and in order, exactly what Path A does: builds the
release binaries, downloads the MinIO server binary from dl.min.io into
`tools/bin/`, starts MinIO and the gateway (active mode) on local ports,
seeds the fleet through the gateway, waits for ingest, runs the scanner, and
prints where the HTML report landed plus the measured through-the-engine SRF.
Everything runs under `./.quickstart/`; stop it all with:

```bash
bash tools/quickstart-native.sh stop
```

## What you just saw

- The fleet is ~200 sandbox snapshots derived from 3 base templates (with
  gzip-compressed variants). Logical size lands around 1.3 GiB; the engine
  stores it in roughly 200 MiB of packs — an SRF above 5.
- `aws s3 ls` shows your objects at full logical size, `_sparta/` is hidden,
  and every GET is BLAKE3-verified against the manifest before the last byte.
- The meter log (`sparta-meter verify` / `report`) is the source of truth for
  savings; nothing left your machine.

## Pointing your own workload at it

1. Deploy the gateway next to your workload:
   - Kubernetes: `helm install sparta deploy/helm/sparta-engine
     --set upstream.endpoint=https://s3.us-east-1.amazonaws.com
     --set credentials.secretName=sparta-credentials`
   - AWS/ECS: `deploy/terraform/sparta-engine` (existing VPC, internal NLB,
     bucket-scoped task role).
2. Start in `mode: shadow` — pure passthrough with savings estimation. Read
   `docs/OPERATIONS.md` §2 (bypass and LB failover) before going further.
3. Change one URL: point your application's S3 endpoint at the gateway.
4. Review shadow numbers (`sparta-gateway stats`, `sparta-meter report`),
   then flip to `mode: active` to start realizing savings on new writes.
   Existing objects are never rewritten.

For a customer-facing redundancy assessment of an existing bucket without
deploying anything, run the read-only scanner alone:

```bash
cargo run --release -p sparta-scanner -- \
  --bucket <your-bucket> --sample-rate 0.01 --out scan-report
```
