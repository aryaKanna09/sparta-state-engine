#!/usr/bin/env bash
# Sparta gateway acceptance workflow, driven by the unmodified `aws s3` CLI.
# Usage: aws_cli_workflow.sh <endpoint> [bucket] [size_mb]
# Credentials come from the standard AWS environment variables.
set -euo pipefail

ENDPOINT=${1:?endpoint required}
BUCKET=${2:-sparta-cli-e2e}
SIZE_MB=${3:-256}
AWS=${AWS_CLI_BIN:-aws}

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

say() { echo "[aws-cli-workflow] $*"; }

say "generating ${SIZE_MB} MiB test file"
# Deterministic-ish compressible+random mix, fast to produce.
python3 - "$workdir/src.bin" "$SIZE_MB" <<'EOF'
import sys
path, mb = sys.argv[1], int(sys.argv[2])
state = 987654321
with open(path, "wb") as f:
    for i in range(mb):
        if i % 3 == 0:
            block = (b"sparta cli workflow %016d " % i) * (1024 * 1024 // 27 + 1)
            f.write(block[: 1024 * 1024])
        else:
            out = bytearray()
            while len(out) < 1024 * 1024:
                state ^= (state << 13) & 0xFFFFFFFFFFFFFFFF
                state ^= state >> 7
                state ^= (state << 17) & 0xFFFFFFFFFFFFFFFF
                out += state.to_bytes(8, "little")
            f.write(out[: 1024 * 1024])
EOF
SRC_SHA=$(shasum -a 256 "$workdir/src.bin" | cut -d' ' -f1)

$AWS --endpoint-url "$ENDPOINT" s3 mb "s3://$BUCKET" 2>/dev/null || true

say "multipart upload via aws s3 cp"
$AWS --endpoint-url "$ENDPOINT" s3 cp "$workdir/src.bin" "s3://$BUCKET/cli/big.bin" --no-progress

say "list"
$AWS --endpoint-url "$ENDPOINT" s3 ls "s3://$BUCKET/cli/" | grep -q big.bin

say "ranged get"
$AWS --endpoint-url "$ENDPOINT" s3api get-object \
    --bucket "$BUCKET" --key cli/big.bin \
    --range bytes=1048576-2097151 "$workdir/slice.bin" >/dev/null
python3 - "$workdir/src.bin" "$workdir/slice.bin" <<'EOF'
import sys
src, sl = sys.argv[1], sys.argv[2]
with open(src, "rb") as f:
    f.seek(1048576)
    want = f.read(1048576)
got = open(sl, "rb").read()
assert got == want, f"ranged bytes differ ({len(got)} vs {len(want)})"
EOF

say "server-side copy"
$AWS --endpoint-url "$ENDPOINT" s3 cp "s3://$BUCKET/cli/big.bin" "s3://$BUCKET/cli/copy.bin" --no-progress

say "download copy and verify"
$AWS --endpoint-url "$ENDPOINT" s3 cp "s3://$BUCKET/cli/copy.bin" "$workdir/copy.bin" --no-progress
COPY_SHA=$(shasum -a 256 "$workdir/copy.bin" | cut -d' ' -f1)
[ "$COPY_SHA" = "$SRC_SHA" ] || { echo "sha mismatch: $COPY_SHA != $SRC_SHA"; exit 1; }

say "delete"
$AWS --endpoint-url "$ENDPOINT" s3 rm "s3://$BUCKET/cli/" --recursive --only-show-errors
if $AWS --endpoint-url "$ENDPOINT" s3 ls "s3://$BUCKET/cli/" | grep -q .; then
    echo "delete left objects behind"; exit 1
fi

echo "CLI WORKFLOW PASS"
