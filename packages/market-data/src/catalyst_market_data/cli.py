"""Backfill CLI for the historical market-data store.

    python -m catalyst_market_data.cli ingest-binance \
        --root data/market-data --venue hyperliquid --symbol ETH \
        --binance-symbol ETHUSDT --interval 1h \
        --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z
"""

from __future__ import annotations

import argparse
from datetime import datetime

from .binance import httpx_transport, ingest_binance
from .parquet_store import ParquetStore


def _dt(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="catalyst-market-data")
    sub = parser.add_subparsers(dest="command", required=True)

    bz = sub.add_parser("ingest-binance", help="Backfill candles from Binance klines")
    bz.add_argument("--root", required=True, help="Parquet store root directory")
    bz.add_argument(
        "--venue", required=True, help="Venue to store candles under (e.g. hyperliquid)"
    )
    bz.add_argument("--symbol", required=True, help="Symbol to store under (e.g. ETH)")
    bz.add_argument("--binance-symbol", required=True, help="Binance pair (e.g. ETHUSDT)")
    bz.add_argument("--interval", required=True, choices=["1m", "5m", "15m", "1h", "4h", "1d"])
    bz.add_argument("--start", required=True, type=_dt)
    bz.add_argument("--end", required=True, type=_dt)

    args = parser.parse_args(argv)

    if args.command == "ingest-binance":
        store = ParquetStore(args.root)
        n = ingest_binance(
            store,
            venue=args.venue,
            symbol=args.symbol,
            binance_symbol=args.binance_symbol,
            interval=args.interval,
            start=args.start,
            end=args.end,
            transport=httpx_transport(),
        )
        print(f"ingested {n} candles into {args.root} ({args.venue}/{args.symbol}/{args.interval})")
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
