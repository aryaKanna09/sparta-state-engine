#!/usr/bin/env python3
"""Sparta gateway acceptance workflow, driven by an unmodified boto3 client.

Exercises the full customer-shaped path through the gateway:
multipart upload, HEAD, ListObjectsV2, full GET, ranged GET, (multipart) copy,
and delete — with byte identity verified by SHA-256 end to end.

Usage:
  boto3_workflow.py --endpoint http://127.0.0.1:8014 [--bucket sparta-e2e]
                    [--size-mb 6144] [--part-mb 64] [--prefix wf]

Credentials come from the standard AWS environment variables.
Exit code 0 and a final "WORKFLOW PASS" line mean success.
"""

import argparse
import hashlib
import os
import sys
import tempfile

import boto3
from boto3.s3.transfer import TransferConfig
from botocore.config import Config


def log(msg):
    print(f"[boto3-workflow] {msg}", flush=True)


def make_test_file(path, size_mb, seed=1234567):
    """Compressible-but-varied content; returns (sha256, first_mb, slice_1m_2m)."""
    h = hashlib.sha256()
    first_mb = b""
    slice_1m = b""
    block_i = 0
    state = seed
    with open(path, "wb") as f:
        remaining = size_mb * 1024 * 1024
        while remaining > 0:
            n = min(1024 * 1024, remaining)
            if block_i % 3 == 0:
                block = (b"sparta state engine workflow %016d " % block_i) * (n // 36 + 1)
                block = block[:n]
            else:
                # xorshift-ish deterministic noise
                out = bytearray()
                while len(out) < n:
                    state ^= (state << 13) & 0xFFFFFFFFFFFFFFFF
                    state ^= state >> 7
                    state ^= (state << 17) & 0xFFFFFFFFFFFFFFFF
                    out += state.to_bytes(8, "little")
                block = bytes(out[:n])
            f.write(block)
            h.update(block)
            if block_i == 0:
                first_mb = block
            if block_i == 1:
                slice_1m = block
            block_i += 1
            remaining -= n
    return h.hexdigest(), first_mb, slice_1m


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(8 * 1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--endpoint", required=True)
    ap.add_argument("--bucket", default="sparta-e2e")
    ap.add_argument("--size-mb", type=int, default=6144)
    ap.add_argument("--part-mb", type=int, default=64)
    ap.add_argument("--prefix", default="wf")
    args = ap.parse_args()

    s3 = boto3.client(
        "s3",
        endpoint_url=args.endpoint,
        config=Config(
            s3={"addressing_style": "path"},
            retries={"max_attempts": 5, "mode": "standard"},
        ),
    )
    transfer = TransferConfig(
        multipart_threshold=args.part_mb * 1024 * 1024,
        multipart_chunksize=args.part_mb * 1024 * 1024,
        max_concurrency=8,
    )

    key = f"{args.prefix}/multipart-{args.size_mb}mb.bin"
    copy_key = f"{args.prefix}/copy-of-multipart.bin"

    try:
        s3.create_bucket(Bucket=args.bucket)
        log(f"created bucket {args.bucket}")
    except s3.exceptions.ClientError as e:
        code = e.response["Error"]["Code"]
        if code not in ("BucketAlreadyOwnedByYou", "BucketAlreadyExists"):
            raise
        log(f"bucket {args.bucket} exists")

    with tempfile.TemporaryDirectory() as td:
        src = os.path.join(td, "src.bin")
        log(f"generating {args.size_mb} MiB test file")
        want_sha, first_mb, slice_1m = make_test_file(src, args.size_mb)
        size = os.path.getsize(src)

        log(f"multipart upload ({args.part_mb} MiB parts)")
        s3.upload_file(src, args.bucket, key, Config=transfer)

        head = s3.head_object(Bucket=args.bucket, Key=key)
        assert head["ContentLength"] == size, (
            f"HEAD size {head['ContentLength']} != {size}"
        )
        log(f"HEAD ok: {size} bytes, etag {head['ETag']}")

        listed = s3.list_objects_v2(Bucket=args.bucket, Prefix=args.prefix + "/")
        keys = [o["Key"] for o in listed.get("Contents", [])]
        assert key in keys, f"{key} missing from ListObjectsV2: {keys}"
        log(f"ListObjectsV2 ok ({len(keys)} keys under {args.prefix}/)")

        rng = s3.get_object(Bucket=args.bucket, Key=key, Range="bytes=1048576-2097151")
        got = rng["Body"].read()
        assert got == slice_1m, "ranged GET bytes differ"
        assert rng["ResponseMetadata"]["HTTPStatusCode"] == 206
        log("ranged GET ok (bytes=1048576-2097151, byte-identical)")

        rng0 = s3.get_object(Bucket=args.bucket, Key=key, Range="bytes=0-1048575")
        assert rng0["Body"].read() == first_mb, "first-MB ranged GET differs"
        log("ranged GET ok (bytes=0-1048575, byte-identical)")

        dst = os.path.join(td, "dst.bin")
        log("full download")
        s3.download_file(args.bucket, key, dst, Config=transfer)
        got_sha = sha256_file(dst)
        assert got_sha == want_sha, f"download SHA {got_sha} != {want_sha}"
        log(f"download ok, sha256 {got_sha}")

        log("copy (managed; multipart above threshold)")
        s3.copy(
            {"Bucket": args.bucket, "Key": key},
            args.bucket,
            copy_key,
            Config=transfer,
        )
        chead = s3.head_object(Bucket=args.bucket, Key=copy_key)
        assert chead["ContentLength"] == size
        dst2 = os.path.join(td, "copy.bin")
        s3.download_file(args.bucket, copy_key, dst2, Config=transfer)
        copy_sha = sha256_file(dst2)
        assert copy_sha == want_sha, f"copy SHA {copy_sha} != {want_sha}"
        log(f"copy ok, sha256 {copy_sha}")

        s3.delete_object(Bucket=args.bucket, Key=key)
        s3.delete_object(Bucket=args.bucket, Key=copy_key)
        after = s3.list_objects_v2(Bucket=args.bucket, Prefix=args.prefix + "/")
        left = [o["Key"] for o in after.get("Contents", [])]
        assert key not in left and copy_key not in left, f"delete left {left}"
        log("delete ok")

    print("WORKFLOW PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
