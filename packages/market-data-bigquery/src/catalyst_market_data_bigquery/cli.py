"""Backfill the market-data store from Google BigQuery public datasets.

    # L1 gas from the public Ethereum blocks table
    catalyst-ingest-bigquery gas --root data/market-data --chain ethereum \
        --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z \
        --gas-units 120000 --eth-price-usd 2500 --project my-gcp-project

    # prices: bring your own SQL (no curated price feed in the public dataset)
    catalyst-ingest-bigquery prices --root data/market-data \
        --venue ethereum --symbol ETH --interval 1h --sql-file eth_hourly.sql \
        --project my-gcp-project

Auth uses Application Default Credentials (run `gcloud auth application-default
login`); the project comes from --project or GOOGLE_CLOUD_PROJECT.
"""

from __future__ import annotations

import argparse
import os
from datetime import datetime

from catalyst_market_data_core import ParquetStore

from .gas import DEFAULT_DATASET, ingest_gas
from .prices import ingest_candles
from .runner import bigquery_runner

_INTERVALS = ["1m", "5m", "15m", "1h", "4h", "1d"]


def _dt(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def _project(args: argparse.Namespace) -> str | None:
    return args.project or os.environ.get("GOOGLE_CLOUD_PROJECT")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="catalyst-ingest-bigquery")
    sub = parser.add_subparsers(dest="command", required=True)

    gas = sub.add_parser("gas", help="Ingest L1 gas from the public Ethereum blocks table")
    gas.add_argument("--root", required=True)
    gas.add_argument("--chain", required=True, help="Chain to store under (e.g. ethereum)")
    gas.add_argument("--start", required=True, type=_dt)
    gas.add_argument("--end", required=True, type=_dt)
    gas.add_argument("--gas-units", type=int, default=120_000)
    gas.add_argument("--eth-price-usd", required=True, help="Constant ETH price for USD scaling")
    gas.add_argument("--dataset", default=DEFAULT_DATASET)
    gas.add_argument("--sql-file", help="Override the built-in gas SQL")
    gas.add_argument("--project", help="GCP project (else GOOGLE_CLOUD_PROJECT)")

    pr = sub.add_parser("prices", help="Ingest candles from a user-supplied SQL query")
    pr.add_argument("--root", required=True)
    pr.add_argument("--venue", required=True)
    pr.add_argument("--symbol", required=True)
    pr.add_argument("--interval", required=True, choices=_INTERVALS)
    pr.add_argument("--sql-file", required=True, help="SQL returning ts/open/high/low/close[/volume]")
    pr.add_argument("--project", help="GCP project (else GOOGLE_CLOUD_PROJECT)")

    args = parser.parse_args(argv)
    store = ParquetStore(args.root)
    runner = bigquery_runner(_project(args))

    if args.command == "gas":
        sql = None
        if args.sql_file:
            with open(args.sql_file) as f:
                sql = f.read()
        n = ingest_gas(
            store, runner, chain=args.chain, start=args.start, end=args.end,
            gas_units=args.gas_units, eth_price_usd=args.eth_price_usd, dataset=args.dataset, sql=sql,
        )
        print(f"ingested {n} gas points ({args.chain}) from BigQuery")
        return 0

    if args.command == "prices":
        with open(args.sql_file) as f:
            sql = f.read()
        n = ingest_candles(
            store, runner, venue=args.venue, symbol=args.symbol, interval=args.interval, sql=sql,
        )
        print(f"ingested {n} candles ({args.venue}/{args.symbol}/{args.interval}) from BigQuery")
        return 0

    return 1


if __name__ == "__main__":
    raise SystemExit(main())
