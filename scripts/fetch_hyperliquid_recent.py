"""Fetch recent Hyperliquid-native perp mark + funding into the store.

The HL `info` API is public/keyless but retention-limited, so this only works
for a *recent* window (roughly the last few months of 1h candles, ~500 funding
points). Stores candles under venue=hyperliquid/symbol=<symbol> with
provenance=native (#38), and funding under the same venue/symbol.

    uv run --with httpx python scripts/fetch_hyperliquid_recent.py --days 30 --symbol ETH
"""

from __future__ import annotations

import argparse
from datetime import UTC, datetime, timedelta

import httpx

from catalyst_market_data.live import HyperliquidSource
from catalyst_market_data_core import ParquetStore

ROOT = "data/market-data"


class HttpPost:
    """POST transport for the HL info endpoint."""

    def post(self, url: str, body: dict):
        return httpx.post(url, json=body, timeout=30).json()


def main() -> int:
    ap = argparse.ArgumentParser(prog="fetch_hyperliquid_recent")
    ap.add_argument("--days", type=int, default=30)
    ap.add_argument("--symbol", default="ETH")
    ap.add_argument("--interval", default="1h")
    args = ap.parse_args()

    end = datetime.now(UTC).replace(minute=0, second=0, microsecond=0)
    start = end - timedelta(days=args.days)
    src = HyperliquidSource(start, end, args.interval, transport=HttpPost())
    candles = src.candles("hyperliquid", args.symbol)
    funding = src.funding("hyperliquid", args.symbol)

    store = ParquetStore(ROOT)
    nc = store.write_candles("hyperliquid", args.symbol, args.interval, candles)
    nf = store.write_funding("hyperliquid", args.symbol, funding)
    store.set_provenance("candles", f"hyperliquid/{args.symbol}", "native")
    span = f"{candles[0].ts}..{candles[-1].ts}" if candles else "(no data)"
    print(f"wrote {nc} candles + {nf} funding -> venue=hyperliquid/{args.symbol} [native]  {span}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
