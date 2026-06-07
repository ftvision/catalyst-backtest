"""Map Dune saved-query rows into the contract series and write the store.

The queries live on Dune (you author them there and pass the numeric ``query_id``).
Each query must return a timestamp column plus the value columns; column names are
configurable so you don't have to match ours exactly. Decimal values are stored as
strings to preserve precision.
"""

from __future__ import annotations

from datetime import UTC, datetime

from catalyst_contracts import Candle
from catalyst_contracts.market_data import GasPoint
from catalyst_market_data_core import ParquetStore

from .client import DuneClient


def parse_ts(value: object) -> datetime:
    """Parse a Dune timestamp cell (ISO, or ``YYYY-MM-DD HH:MM:SS.fff UTC``)."""

    if isinstance(value, datetime):
        return value if value.tzinfo else value.replace(tzinfo=UTC)
    s = str(value).strip().replace(" UTC", "+00:00").replace("Z", "+00:00")
    parsed = datetime.fromisoformat(s)
    return parsed if parsed.tzinfo else parsed.replace(tzinfo=UTC)


def _dune_dt(ts: datetime) -> str:
    # Dune Datetime parameters expect "YYYY-MM-DD HH:MM:SS" (UTC), not ISO-8601
    # with a "T"/offset, so format explicitly.
    return ts.astimezone(UTC).strftime("%Y-%m-%d %H:%M:%S")


def _window(start: datetime, end: datetime, extra: dict | None) -> dict:
    params = {"start": _dune_dt(start), "end": _dune_dt(end)}
    if extra:
        params.update(extra)
    return params


def fetch_gas(
    client: DuneClient,
    query_id: int,
    *,
    start: datetime,
    end: datetime,
    ts_col: str = "ts",
    gas_col: str = "gas_usd",
    params: dict | None = None,
) -> list[GasPoint]:
    rows = client.run_query(query_id, _window(start, end, params))
    return [GasPoint(ts=parse_ts(r[ts_col]), gas_usd=str(r[gas_col])) for r in rows]


def fetch_candles(
    client: DuneClient,
    query_id: int,
    *,
    start: datetime,
    end: datetime,
    ts_col: str = "ts",
    open_col: str = "open",
    high_col: str = "high",
    low_col: str = "low",
    close_col: str = "close",
    volume_col: str | None = "volume",
    params: dict | None = None,
) -> list[Candle]:
    rows = client.run_query(query_id, _window(start, end, params))
    candles: list[Candle] = []
    for r in rows:
        volume = str(r[volume_col]) if volume_col and volume_col in r and r[volume_col] is not None else None
        candles.append(
            Candle(
                ts=parse_ts(r[ts_col]),
                open=str(r[open_col]),
                high=str(r[high_col]),
                low=str(r[low_col]),
                close=str(r[close_col]),
                volume=volume,
            )
        )
    return candles


def ingest_gas(
    store: ParquetStore, client: DuneClient, *, chain: str, query_id: int, **kwargs
) -> int:
    points = fetch_gas(client, query_id, **kwargs)
    return store.write_gas(chain, points)


def ingest_candles(
    store: ParquetStore,
    client: DuneClient,
    *,
    venue: str,
    symbol: str,
    interval: str,
    query_id: int,
    **kwargs,
) -> int:
    candles = fetch_candles(client, query_id, **kwargs)
    return store.write_candles(venue, symbol, interval, candles)


__all__ = ["fetch_gas", "fetch_candles", "ingest_gas", "ingest_candles", "parse_ts"]
