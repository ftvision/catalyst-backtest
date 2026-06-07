"""Binance klines ingester — a free, deep-history source for reference prices.

Binance ``klines`` returns OHLCV back to ~2017 at 1m granularity, keyless. We use
it as a *reference* price for an asset (e.g. ETH), normalized into our ``Candle``
contract and written to the Parquet store. The HTTP transport is injected so
fetching is testable offline; a real one is created lazily from ``httpx``.

Accuracy note: a CEX reference price is an approximation for a venue like
Hyperliquid (whose own mark drives funding/liquidation). Fine for directional v1.
"""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any, Callable

from catalyst_contracts import Candle

from catalyst_market_data_core import ParquetStore

# Public market-data mirror: same API shape as api.binance.com but keyless and
# not geo-restricted (api.binance.com returns 451 from many regions).
BINANCE_KLINES_URL = "https://data-api.binance.vision/api/v3/klines"
_MAX_LIMIT = 1000

# Binance uses the same interval strings we do.
_INTERVAL_MS = {
    "1m": 60_000,
    "5m": 300_000,
    "15m": 900_000,
    "1h": 3_600_000,
    "4h": 14_400_000,
    "1d": 86_400_000,
}

# transport(url, params) -> parsed JSON (list of kline arrays)
Transport = Callable[[str, dict], Any]


def httpx_transport(timeout: float = 30.0) -> Transport:
    """A real GET transport backed by httpx (imported lazily)."""

    import httpx

    def _get(url: str, params: dict) -> Any:
        return httpx.get(url, params=params, timeout=timeout).json()

    return _get


def _network_disabled(url: str, params: dict) -> Any:  # noqa: ARG001
    raise RuntimeError("no transport configured; pass one (e.g. httpx_transport())")


def _ms(ts: datetime) -> int:
    return int(ts.timestamp() * 1000)


def fetch_klines(
    binance_symbol: str,
    interval: str,
    start: datetime,
    end: datetime,
    transport: Transport | None = None,
) -> list[Candle]:
    """Fetch and normalize klines for ``[start, end]``, paginating as needed."""

    if interval not in _INTERVAL_MS:
        raise ValueError(f"unsupported interval {interval!r}")
    transport = transport or _network_disabled
    step = _INTERVAL_MS[interval]
    end_ms = _ms(end)
    cursor = _ms(start)
    out: list[Candle] = []
    seen: set[int] = set()

    while cursor <= end_ms:
        rows = (
            transport(
                BINANCE_KLINES_URL,
                {
                    "symbol": binance_symbol,
                    "interval": interval,
                    "startTime": cursor,
                    "endTime": end_ms,
                    "limit": _MAX_LIMIT,
                },
            )
            or []
        )
        if not rows:
            break
        if not isinstance(rows, list):
            # Binance signals errors (e.g. 451 geo-block) as a JSON object, not a
            # list of klines — surface it clearly instead of mis-parsing.
            msg = rows.get("msg") if isinstance(rows, dict) else rows
            raise RuntimeError(f"unexpected Binance response (not klines): {msg}")
        for row in rows:
            open_ms = int(row[0])
            if open_ms in seen or open_ms > end_ms:
                continue
            seen.add(open_ms)
            out.append(
                Candle(
                    ts=datetime.fromtimestamp(open_ms / 1000, tz=UTC),
                    open=str(row[1]),
                    high=str(row[2]),
                    low=str(row[3]),
                    close=str(row[4]),
                    volume=str(row[5]),
                )
            )
        last_open = int(rows[-1][0])
        if len(rows) < _MAX_LIMIT:
            break
        cursor = last_open + step  # advance past the last bar

    out.sort(key=lambda c: c.ts)
    return out


def ingest_binance(
    store: ParquetStore,
    *,
    venue: str,
    symbol: str,
    binance_symbol: str,
    interval: str,
    start: datetime,
    end: datetime,
    transport: Transport | None = None,
) -> int:
    """Fetch klines and write them to the store under (venue, symbol, interval)."""

    candles = fetch_klines(binance_symbol, interval, start, end, transport)
    n = store.write_candles(venue, symbol, interval, candles)
    # Binance is a CEX *reference* proxy, not the venue's native price (#38).
    store.set_provenance("candles", f"{venue}/{symbol}", "reference")
    return n


__all__ = ["fetch_klines", "ingest_binance", "httpx_transport", "BINANCE_KLINES_URL", "Transport"]
