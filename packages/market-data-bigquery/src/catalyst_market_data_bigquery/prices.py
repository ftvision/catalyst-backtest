"""Candle ingestion from a user-supplied BigQuery SQL ("bring your own query").

Unlike gas, the public Ethereum dataset has **no curated USD price feed** — DEX
prices require decoding swap events, which is dataset- and pool-specific. Rather
than ship a fragile default, the prices path runs SQL *you* provide (via
`--sql-file`) and maps its columns to candles. The query must return a timestamp
column plus open/high/low/close (and optionally volume).
"""

from __future__ import annotations

from catalyst_contracts import Candle
from catalyst_market_data_core import ParquetStore

from .gas import _ts
from .runner import QueryRunner


def fetch_candles(
    runner: QueryRunner,
    sql: str,
    *,
    ts_col: str = "ts",
    open_col: str = "open",
    high_col: str = "high",
    low_col: str = "low",
    close_col: str = "close",
    volume_col: str | None = "volume",
) -> list[Candle]:
    rows = runner(sql)
    candles: list[Candle] = []
    for r in rows:
        has_vol = volume_col and volume_col in r and r[volume_col] is not None
        candles.append(
            Candle(
                ts=_ts(r[ts_col]),
                open=str(r[open_col]),
                high=str(r[high_col]),
                low=str(r[low_col]),
                close=str(r[close_col]),
                volume=str(r[volume_col]) if has_vol else None,
            )
        )
    return candles


def ingest_candles(
    store: ParquetStore,
    runner: QueryRunner,
    *,
    venue: str,
    symbol: str,
    interval: str,
    sql: str,
    **kwargs,
) -> int:
    return store.write_candles(venue, symbol, interval, fetch_candles(runner, sql, **kwargs))


__all__ = ["fetch_candles", "ingest_candles"]
