"""Upload the local Parquet market-data store to a Cloudflare R2 (S3) bucket.

Run with boto3 available, e.g.:

    AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... \
    uv run --with boto3 python scripts/upload_r2.py \
        --bucket catalyst-market-data --prefix market-data \
        --endpoint https://<ACCOUNT_ID>.r2.cloudflarestorage.com \
        --root data/market-data

Credentials are read from the environment (never passed on the CLI). The remote
key layout mirrors the local tree, so the Rust loader reads it with
CATALYST_STORE_ROOT=s3://<bucket>/<prefix>.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser(prog="upload_r2")
    ap.add_argument("--bucket", required=True)
    ap.add_argument("--prefix", default="market-data", help="key prefix inside the bucket")
    ap.add_argument("--endpoint", required=True, help="R2 S3 endpoint URL")
    ap.add_argument("--root", default="data/market-data", help="local store root")
    ap.add_argument("--region", default="auto")
    args = ap.parse_args()

    if not os.environ.get("AWS_ACCESS_KEY_ID") or not os.environ.get("AWS_SECRET_ACCESS_KEY"):
        raise SystemExit("set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY in the environment")

    import boto3

    s3 = boto3.client("s3", endpoint_url=args.endpoint, region_name=args.region)
    root = Path(args.root)
    files = sorted(p for p in root.rglob("*") if p.is_file())
    if not files:
        raise SystemExit(f"no files under {root}")

    n = 0
    for f in files:
        key = f"{args.prefix}/{f.relative_to(root).as_posix()}"
        s3.upload_file(str(f), args.bucket, key)
        n += 1
        print(f"  uploaded {key}")
    print(f"done: {n} objects -> s3://{args.bucket}/{args.prefix}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
