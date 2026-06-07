"""Backfill the market-data store from Dune saved queries.

    # gas (the query must return ts + gas_usd columns)
    catalyst-ingest-dune gas --root data/market-data --chain ethereum \
        --query-id 1234567 --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z

    # prices/candles (ts + open/high/low/close[/volume])
    catalyst-ingest-dune prices --root data/market-data \
        --venue ethereum --symbol ETH --interval 1h --query-id 7654321 \
        --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z

The Dune API key is read from --api-key or the DUNE_API_KEY environment variable.
"""

from __future__ import annotations

import argparse
import os
from datetime import datetime

from catalyst_market_data_core import ParquetStore, http_transport

from .client import DuneClient
from .ingest import ingest_candles, ingest_gas

_INTERVALS = ["1m", "5m", "15m", "1h", "4h", "1d"]


def _dt(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def _kv(value: str) -> tuple[str, str]:
    key, _, val = value.partition("=")
    if not key or not val:
        raise argparse.ArgumentTypeError(f"expected key=value, got {value!r}")
    return key, val


def _api_key(args: argparse.Namespace) -> str:
    key = args.api_key or os.environ.get("DUNE_API_KEY")
    if not key:
        raise SystemExit("no Dune API key: pass --api-key or set DUNE_API_KEY")
    return key


def _common(p: argparse.ArgumentParser) -> None:
    p.add_argument("--root", required=True, help="Parquet store root directory")
    p.add_argument("--query-id", required=True, type=int, help="Dune saved query id")
    p.add_argument("--start", required=True, type=_dt)
    p.add_argument("--end", required=True, type=_dt)
    p.add_argument("--api-key", help="Dune API key (else DUNE_API_KEY)")
    p.add_argument("--ts-col", default="ts", help="Timestamp column name in the query result")
    p.add_argument(
        "--param", action="append", type=_kv, default=[], metavar="KEY=VALUE",
        help="Extra Dune query parameter (repeatable)",
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="catalyst-ingest-dune")
    sub = parser.add_subparsers(dest="command", required=True)

    gas = sub.add_parser("gas", help="Ingest a per-chain gas series from a Dune query")
    _common(gas)
    gas.add_argument("--chain", required=True, help="Chain to store under (e.g. ethereum)")
    gas.add_argument("--gas-col", default="gas_usd", help="USD-gas column name")

    pr = sub.add_parser("prices", help="Ingest a candle series from a Dune query")
    _common(pr)
    pr.add_argument("--venue", required=True)
    pr.add_argument("--symbol", required=True)
    pr.add_argument("--interval", required=True, choices=_INTERVALS)
    pr.add_argument("--open-col", default="open")
    pr.add_argument("--high-col", default="high")
    pr.add_argument("--low-col", default="low")
    pr.add_argument("--close-col", default="close")
    pr.add_argument("--volume-col", default="volume")

    args = parser.parse_args(argv)
    store = ParquetStore(args.root)
    client = DuneClient(_api_key(args), http_transport())
    params = dict(args.param)

    if args.command == "gas":
        n = ingest_gas(
            store, client, chain=args.chain, query_id=args.query_id,
            start=args.start, end=args.end, ts_col=args.ts_col, gas_col=args.gas_col, params=params,
        )
        print(f"ingested {n} gas points ({args.chain}) from Dune query {args.query_id}")
        return 0

    if args.command == "prices":
        n = ingest_candles(
            store, client, venue=args.venue, symbol=args.symbol, interval=args.interval,
            query_id=args.query_id, start=args.start, end=args.end, ts_col=args.ts_col,
            open_col=args.open_col, high_col=args.high_col, low_col=args.low_col,
            close_col=args.close_col, volume_col=args.volume_col, params=params,
        )
        print(f"ingested {n} candles ({args.venue}/{args.symbol}/{args.interval}) from Dune")
        return 0

    return 1


if __name__ == "__main__":
    raise SystemExit(main())
