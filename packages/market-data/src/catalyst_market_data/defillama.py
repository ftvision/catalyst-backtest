"""DefiLlama yield (APY) ingester — a free, historical Aave yield source.

DefiLlama's ``yields.llama.fi/chart/{pool_id}`` returns historical APY for a pool,
keyless. We normalize it into our ``YieldPoint`` contract (APR as a *fraction*,
e.g. 4.5% -> "0.045", matching how the engine accrues) and write it to the
Parquet store. The HTTP transport is injected (reuse ``httpx_transport``), so
fetching is testable offline.

A pool is identified by DefiLlama's UUID (from ``yields.llama.fi/pools``); the
*storage* pool label (e.g. ``usdc``) is separate and must match the graph config.
"""

from __future__ import annotations

from datetime import UTC, datetime
from decimal import Decimal

from catalyst_contracts.market_data import YieldPoint

from .binance import Transport
from catalyst_market_data_core import ParquetStore

YIELDS_CHART_URL = "https://yields.llama.fi/chart"


def _parse_ts(value) -> datetime:
    if isinstance(value, (int, float)):
        return datetime.fromtimestamp(value, tz=UTC)
    return datetime.fromisoformat(str(value).replace("Z", "+00:00"))


def fetch_aave_yields(
    pool_id: str,
    start: datetime,
    end: datetime,
    transport: Transport,
) -> list[YieldPoint]:
    """Fetch a pool's historical APY and normalize to ``YieldPoint`` (APR fraction)."""

    payload = transport(f"{YIELDS_CHART_URL}/{pool_id}", None)
    if not isinstance(payload, dict) or not isinstance(payload.get("data"), list):
        raise RuntimeError(f"unexpected DefiLlama yields response for pool {pool_id!r}")

    points: list[YieldPoint] = []
    for row in payload["data"]:
        ts = _parse_ts(row["timestamp"])
        if ts < start or ts > end:
            continue
        apy = row.get("apy")
        if apy is None:
            continue
        apr = Decimal(str(apy)) / Decimal(100)  # percent -> fraction
        points.append(YieldPoint(ts=ts, apr=str(apr)))
    points.sort(key=lambda p: p.ts)
    return points


def ingest_aave_yields(
    store: ParquetStore,
    *,
    asset: str,
    chain: str,
    pool: str,
    pool_id: str,
    start: datetime,
    end: datetime,
    transport: Transport,
    protocol: str = "aave",
) -> int:
    """Fetch DefiLlama APY for ``pool_id`` and store it under (protocol, asset, chain, pool)."""

    points = fetch_aave_yields(pool_id, start, end, transport)
    return store.write_yields(protocol, asset, chain, pool, points)


__all__ = ["fetch_aave_yields", "ingest_aave_yields", "YIELDS_CHART_URL"]
