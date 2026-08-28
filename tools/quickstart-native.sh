#!/usr/bin/env bash
# Sparta State Engine — native quickstart (no Docker).
#
# Builds the release binaries, brings up MinIO + the gateway (active mode)
# locally, runs the customer journey end to end:
#   1. seed a RAW copy of the simulated agent fleet -> scan it (Shadow A,
#      the "what could you save" SRF-S report)
#   2. seed the same fleet THROUGH the gateway -> engine ingests it
#   3. print the verified through-the-engine SRF
#
# Everything lives under ./.quickstart/. `bash tools/quickstart-native.sh stop`
# tears it down.

set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
QS="$ROOT/.quickstart"
MINIO_PORT=19000
GW_PORT=18014
export AWS_ACCESS_KEY_ID=sparta-test AWS_SECRET_ACCESS_KEY=sparta-test-secret AWS_REGION=us-east-1

say() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

stop_all() {
    for pid_file in "$QS"/minio.pid "$QS"/gateway.pid; do
        if [ -f "$pid_file" ]; then
            kill "$(cat "$pid_file")" 2>/dev/null || true
            rm -f "$pid_file"
        fi
    done
    echo "stopped."
}

if [ "${1:-}" = "stop" ]; then
    stop_all
    exit 0
fi

trap 'echo; echo "quickstart failed — logs are in $QS/*.log"; exit 1' ERR
mkdir -p "$QS/minio-data" "$QS/state" "$QS/cache" tools/bin
START_TS=$(date +%s)

say "Building release binaries (first build takes a few minutes)"
cargo build --release -q -p sparta-gateway -p sparta-scanner -p sparta-meter -p sparta-testkit
BIN="$ROOT/target/release"

say "Fetching MinIO server binary (official dl.min.io release)"
if [ ! -x tools/bin/minio ]; then
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64)  MINIO_URL=https://dl.min.io/server/minio/release/darwin-arm64/minio ;;
        Darwin-x86_64) MINIO_URL=https://dl.min.io/server/minio/release/darwin-amd64/minio ;;
        Linux-x86_64)  MINIO_URL=https://dl.min.io/server/minio/release/linux-amd64/minio ;;
        Linux-aarch64) MINIO_URL=https://dl.min.io/server/minio/release/linux-arm64/minio ;;
        *) echo "unsupported platform $(uname -s)-$(uname -m)"; exit 1 ;;
    esac
    curl -sSfL -o tools/bin/minio "$MINIO_URL"
    chmod +x tools/bin/minio
fi

say "Starting MinIO on :$MINIO_PORT (data under .quickstart/minio-data)"
if [ ! -f "$QS/minio.pid" ] || ! kill -0 "$(cat "$QS/minio.pid")" 2>/dev/null; then
    MINIO_ROOT_USER=$AWS_ACCESS_KEY_ID MINIO_ROOT_PASSWORD=$AWS_SECRET_ACCESS_KEY \
        tools/bin/minio server "$QS/minio-data" --address "127.0.0.1:$MINIO_PORT" \
        >"$QS/minio.log" 2>&1 &
    echo $! > "$QS/minio.pid"
fi
for _ in $(seq 1 60); do
    curl -sf "http://127.0.0.1:$MINIO_PORT/minio/health/live" >/dev/null && break
    sleep 0.5
done

say "Starting the Sparta gateway (active mode) on :$GW_PORT"
if [ ! -f "$QS/gateway.pid" ] || ! kill -0 "$(cat "$QS/gateway.pid")" 2>/dev/null; then
    SPARTA_LISTEN="127.0.0.1:$GW_PORT" \
    SPARTA_UPSTREAM_ENDPOINT="http://127.0.0.1:$MINIO_PORT" \
    SPARTA_ACCESS_KEY=$AWS_ACCESS_KEY_ID SPARTA_SECRET_KEY=$AWS_SECRET_ACCESS_KEY \
    SPARTA_MODE=active \
    SPARTA_DB_PATH="$QS/state/gateway.sqlite" \
    SPARTA_METER_PATH="$QS/state/meter.jsonl" \
    SPARTA_CACHE_DIR="$QS/cache" \
    RUST_LOG=info \
        "$BIN/sparta-gateway" serve >"$QS/gateway.log" 2>&1 &
    echo $! > "$QS/gateway.pid"
fi
for _ in $(seq 1 60); do
    curl -sf "http://127.0.0.1:$GW_PORT/readyz" >/dev/null && break
    sleep 0.5
done

say "Step 1 — Shadow A: seed a RAW fleet copy and scan it"
"$BIN/sparta-fixtures" --endpoint "http://127.0.0.1:$MINIO_PORT" --bucket fleet-raw \
    fleet --snapshots 200 | sed 's/^/  /'
"$BIN/sparta-scanner" --bucket fleet-raw --endpoint "http://127.0.0.1:$MINIO_PORT" \
    --sample-rate 1.0 --out "$QS/scan-report" 2>"$QS/scanner.log" | sed 's/^/  /'

say "Step 2 — Activation: seed the same fleet THROUGH the gateway"
"$BIN/sparta-fixtures" --endpoint "http://127.0.0.1:$GW_PORT" --bucket fleet \
    fleet --snapshots 200 | sed 's/^/  /'
printf '  waiting for background ingest'
stats() {
    SPARTA_DB_PATH="$QS/state/gateway.sqlite" SPARTA_UPSTREAM_ENDPOINT=http://unused:1 \
        "$BIN/sparta-gateway" stats
}
for _ in $(seq 1 240); do
    QUEUE=$(stats | python3 -c 'import json,sys; print(json.load(sys.stdin)["active"]["ingest_queue"])')
    [ "$QUEUE" = "0" ] && break
    printf '.'
    sleep 1
done
echo " done"

say "Results"
stats | python3 -c '
import json, sys
s = json.load(sys.stdin)["active"]
gib = 1024 ** 3
logical = s["logical_stored_bytes"] / gib
physical = s["physical_stored_bytes"] / gib
print("  logical stored through engine : %.2f GiB" % logical)
print("  physical stored (packs+meta)  : %.2f GiB" % physical)
print("  verified SRF through engine   : %.2fx" % s["srf"])
'
SRF_LINE=$(grep -o 'SRF-S [0-9.]*x' "$QS"/scanner.log 2>/dev/null | head -1 || true)
echo "  Shadow A scan report          : $QS/scan-report/report.html"
"$BIN/sparta-meter" --log "$QS/state/meter.jsonl" verify | sed 's/^/  meter: /'

ELAPSED=$(( $(date +%s) - START_TS ))
echo
echo "Quickstart complete in ${ELAPSED}s."
echo "Gateway S3 endpoint: http://127.0.0.1:$GW_PORT  (creds: $AWS_ACCESS_KEY_ID / $AWS_SECRET_ACCESS_KEY)"
echo "Try:  aws --endpoint-url http://127.0.0.1:$GW_PORT s3 ls s3://fleet/snapshots/"
echo "Stop: bash tools/quickstart-native.sh stop"
