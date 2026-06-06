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
from .defillama import ingest_aave_yields
from .evm_gas import httpx_rpc_transport, ingest_constant_gas, ingest_recent_gas
from .parquet_store import ParquetStore

_INTERVALS = ["1m", "5m", "15m", "1h", "4h", "1d"]


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
    bz.add_argument("--interval", required=True, choices=_INTERVALS)
    bz.add_argument("--start", required=True, type=_dt)
    bz.add_argument("--end", required=True, type=_dt)

    ay = sub.add_parser("ingest-aave-yields", help="Backfill Aave APY from DefiLlama")
    ay.add_argument("--root", required=True)
    ay.add_argument("--asset", required=True, help="Asset symbol (e.g. USDC)")
    ay.add_argument("--chain", required=True, help="Chain (e.g. base)")
    ay.add_argument("--pool", required=True, help="Storage pool label (e.g. usdc)")
    ay.add_argument("--pool-id", required=True, help="DefiLlama pool UUID")
    ay.add_argument("--start", required=True, type=_dt)
    ay.add_argument("--end", required=True, type=_dt)

    gas = sub.add_parser("ingest-gas", help="Backfill chain gas (recent RPC, or constant estimate)")
    gas.add_argument("--root", required=True)
    gas.add_argument("--chain", required=True, help="Chain (e.g. base)")
    gas.add_argument("--constant", help="Flat gas_usd estimate over the window")
    gas.add_argument("--interval", default="1h", choices=_INTERVALS)
    gas.add_argument("--start", type=_dt, help="Window start (constant mode)")
    gas.add_argument("--end", type=_dt, help="Window end (constant mode)")
    gas.add_argument("--rpc-url", help="JSON-RPC URL for recent eth_feeHistory mode")
    gas.add_argument("--block-count", type=int, default=300)
    gas.add_argument("--gas-units", type=int, default=120_000)
    gas.add_argument("--eth-price-usd", help="ETH price used to convert base fee to USD")

    args = parser.parse_args(argv)
    store = ParquetStore(args.root)

    if args.command == "ingest-binance":
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
        print(f"ingested {n} candles ({args.venue}/{args.symbol}/{args.interval})")
        return 0

    if args.command == "ingest-aave-yields":
        n = ingest_aave_yields(
            store,
            asset=args.asset,
            chain=args.chain,
            pool=args.pool,
            pool_id=args.pool_id,
            start=args.start,
            end=args.end,
            transport=httpx_transport(),
        )
        print(f"ingested {n} yield points (aave/{args.asset} on {args.chain})")
        return 0

    if args.command == "ingest-gas":
        if args.constant is not None:
            n = ingest_constant_gas(
                store,
                chain=args.chain,
                start=args.start,
                end=args.end,
                interval=args.interval,
                gas_usd=args.constant,
            )
            print(f"ingested {n} constant gas points ({args.chain} @ {args.constant} USD)")
        else:
            n = ingest_recent_gas(
                store,
                chain=args.chain,
                rpc_url=args.rpc_url,
                block_count=args.block_count,
                gas_units=args.gas_units,
                eth_price_usd=args.eth_price_usd,
                transport=httpx_rpc_transport(),
            )
            print(f"ingested {n} recent gas points ({args.chain})")
        return 0

    return 1


if __name__ == "__main__":
    raise SystemExit(main())
