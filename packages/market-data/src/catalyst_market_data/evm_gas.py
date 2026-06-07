"""EVM gas ingester for the Parquet store.

Two ways to populate the per-chain gas series:

- :func:`fetch_recent_gas` — **real** recent base fees via JSON-RPC
  ``eth_feeHistory`` on a chain's RPC (e.g. Base), converted to a USD-per-action
  estimate. This is **recent-only** (``eth_feeHistory`` covers ~hundreds of
  blocks), so it cannot backfill deep history.
- :func:`constant_gas_series` — a flat USD estimate across a backtest window, for
  backfilling historical gas when real per-tick history isn't available.

`gas_usd` = ``base_fee_wei * gas_units / 1e18 * eth_price_usd`` — a documented
approximation (priority fee and exact per-tx gas units are not modeled).

The JSON-RPC transport is injected, so both are testable offline.
"""

from __future__ import annotations

from datetime import UTC, datetime, timedelta
from decimal import Decimal
from typing import Any, Callable

from catalyst_contracts.market_data import GasPoint

from catalyst_market_data_core import ParquetStore

# transport(rpc_url, method, params) -> JSON-RPC "result"
RpcTransport = Callable[[str, str, list], Any]

_INTERVAL_SECONDS = {"1m": 60, "5m": 300, "15m": 900, "1h": 3600, "4h": 14400, "1d": 86400}


def httpx_rpc_transport(timeout: float = 30.0) -> RpcTransport:
    """A real JSON-RPC transport backed by httpx (imported lazily)."""

    import httpx

    def _rpc(url: str, method: str, params: list) -> Any:
        body = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
        data = httpx.post(url, json=body, timeout=timeout).json()
        if "error" in data:
            raise RuntimeError(f"RPC error for {method}: {data['error']}")
        return data.get("result")

    return _rpc


def _gas_usd(base_fee_wei: int, gas_units: int, eth_price_usd: Decimal) -> str:
    value = Decimal(base_fee_wei) * Decimal(gas_units) / Decimal(10**18) * eth_price_usd
    return str(value)


def fetch_recent_gas(
    rpc_url: str,
    *,
    block_count: int,
    gas_units: int,
    eth_price_usd: Decimal | str,
    transport: RpcTransport,
    block_time_seconds: int = 2,
) -> list[GasPoint]:
    """Recent per-block gas estimate from ``eth_feeHistory`` (recent-only)."""

    eth_price = Decimal(str(eth_price_usd))
    fee = transport(rpc_url, "eth_feeHistory", [hex(block_count), "latest", []])
    if not isinstance(fee, dict) or "baseFeePerGas" not in fee:
        raise RuntimeError("unexpected eth_feeHistory response")
    base_fees = [int(x, 16) for x in fee["baseFeePerGas"]]
    oldest = int(fee["oldestBlock"], 16)

    # eth_feeHistory has no timestamps; anchor on the oldest block's timestamp and
    # advance by the chain's block time.
    block = transport(rpc_url, "eth_getBlockByNumber", [hex(oldest), False])
    oldest_ts = int(block["timestamp"], 16)

    points: list[GasPoint] = []
    for i, base_fee in enumerate(base_fees[:-1]):  # last entry is the next-block forecast
        ts = datetime.fromtimestamp(oldest_ts + i * block_time_seconds, tz=UTC)
        points.append(GasPoint(ts=ts, gas_usd=_gas_usd(base_fee, gas_units, eth_price)))
    return points


def constant_gas_series(
    start: datetime, end: datetime, interval: str, gas_usd: Decimal | str
) -> list[GasPoint]:
    """A flat gas estimate at each interval tick across ``[start, end]``."""

    if interval not in _INTERVAL_SECONDS:
        raise ValueError(f"unsupported interval {interval!r}")
    step = timedelta(seconds=_INTERVAL_SECONDS[interval])
    value = str(Decimal(str(gas_usd)))
    points: list[GasPoint] = []
    ts = start
    while ts <= end:
        points.append(GasPoint(ts=ts, gas_usd=value))
        ts += step
    return points


def ingest_recent_gas(store: ParquetStore, *, chain: str, **kwargs) -> int:
    points = fetch_recent_gas(**kwargs)
    return store.write_gas(chain, points)


def ingest_constant_gas(
    store: ParquetStore,
    *,
    chain: str,
    start: datetime,
    end: datetime,
    interval: str,
    gas_usd: Decimal | str,
) -> int:
    points = constant_gas_series(start, end, interval, gas_usd)
    return store.write_gas(chain, points)


__all__ = [
    "fetch_recent_gas",
    "constant_gas_series",
    "ingest_recent_gas",
    "ingest_constant_gas",
    "httpx_rpc_transport",
    "RpcTransport",
]
