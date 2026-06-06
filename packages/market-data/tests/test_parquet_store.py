"""Tests for the Parquet historical store, ParquetSource, and Binance ingester."""

from __future__ import annotations

from datetime import UTC, datetime

import pytest

from catalyst_contracts import Candle
from catalyst_contracts.market_data import FundingPoint, GasPoint, YieldPoint
from catalyst_graph_compiler import compile_graph
from catalyst_market_data import (
    ParquetSource,
    ParquetStore,
    build_bundle,
    fetch_klines,
    ingest_binance,
)


def dt(day: int, hour: int = 0) -> datetime:
    return datetime(2024, 1, day, hour, tzinfo=UTC)


def candle(day: int, hour: int, close: str) -> Candle:
    return Candle(ts=dt(day, hour), open=close, high=close, low=close, close=close, volume="1")


# --- store round-trip + partition/window read ---


def test_candles_round_trip(tmp_path) -> None:
    store = ParquetStore(tmp_path)
    store.write_candles("base", "ETH", "1h", [candle(1, 0, "2000"), candle(1, 1, "2010")])
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h")
    candles = src.candles("base", "ETH")
    assert [c.close for c in candles] == ["2000", "2010"]
    assert candles[0].ts == dt(1, 0)


def test_window_and_partition_filtering(tmp_path) -> None:
    store = ParquetStore(tmp_path)
    # three days of hourly data
    store.write_candles(
        "base",
        "ETH",
        "1h",
        [candle(d, h, str(2000 + d)) for d in (1, 2, 3) for h in range(24)],
    )
    # ask only for day 2
    src = ParquetSource(tmp_path, dt(2, 0), dt(2, 23), "1h")
    candles = src.candles("base", "ETH")
    assert len(candles) == 24
    assert {c.close for c in candles} == {"2002"}


def test_incremental_write_merges_by_ts(tmp_path) -> None:
    store = ParquetStore(tmp_path)
    store.write_candles("base", "ETH", "1h", [candle(1, 0, "2000")])
    store.write_candles("base", "ETH", "1h", [candle(1, 1, "2010")])  # same day, appended
    store.write_candles("base", "ETH", "1h", [candle(1, 0, "1999")])  # overwrite ts 0
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h")
    candles = src.candles("base", "ETH")
    assert [(c.ts.hour, c.close) for c in candles] == [(0, "1999"), (1, "2010")]


def test_funding_gas_yields_round_trip(tmp_path) -> None:
    store = ParquetStore(tmp_path)
    store.write_funding("hyperliquid", "ETH", [FundingPoint(ts=dt(1, 0), rate="0.0001")])
    store.write_gas("base", [GasPoint(ts=dt(1, 0), gas_usd="0.02")])
    store.write_yields("aave", "USDC", "base", "usdc", [YieldPoint(ts=dt(1, 0), apr="0.045")])
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h")
    assert src.funding("hyperliquid", "ETH")[0].rate == "0.0001"
    assert src.gas("base")[0].gas_usd == "0.02"
    assert src.yields("aave", "USDC", "base", "usdc")[0].apr == "0.045"


def test_missing_series_returns_empty(tmp_path) -> None:
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h")
    assert src.candles("base", "DOGE") == []


def test_coverage(tmp_path) -> None:
    store = ParquetStore(tmp_path)
    store.write_candles("base", "ETH", "1h", [candle(1, 0, "2000"), candle(3, 5, "2100")])
    cov = store.coverage(store._candle_dir("base", "ETH", "1h"))
    assert cov == (dt(1, 0), dt(3, 5))


# --- integrates with the planner like any other source ---


def test_build_bundle_from_parquet_source(tmp_path) -> None:
    store = ParquetStore(tmp_path)
    store.write_candles("base", "ETH", "1h", [candle(1, 0, "2000"), candle(1, 1, "2010")])
    store.write_gas("base", [GasPoint(ts=dt(1, 0), gas_usd="0.02")])
    graph = {
        "nodes": [
            {
                "id": "buy",
                "kind": "action",
                "subtype": "swap",
                "config": {
                    "from_asset": "USDC",
                    "to_asset": "ETH",
                    "amount": "100",
                    "chain": "base",
                },
            }
        ],
        "edges": [],
    }
    bundle = build_bundle(
        compile_graph(graph),
        start=dt(1, 0),
        end=dt(1, 2),
        interval="1h",
        source=ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h"),
    )
    assert {(s.venue, s.symbol) for s in bundle.candles} == {("base", "ETH")}
    assert bundle.candles[0].points
    assert bundle.warnings == []
    assert all(p.name == "parquet-store" for p in bundle.providers)


# --- Binance ingester (offline via fake transport) ---


def fake_klines(rows):
    def transport(url, params):
        # honor pagination: return rows whose openTime >= startTime, capped at limit
        start = params["startTime"]
        page = [r for r in rows if r[0] >= start][: params["limit"]]
        return page

    return transport


def test_fetch_klines_parses_and_paginates() -> None:
    # 2 hourly bars on 2024-01-01
    rows = [
        [1704067200000, "2000", "2010", "1990", "2005", "12"],
        [1704070800000, "2005", "2050", "1980", "1990", "18"],
    ]
    candles = fetch_klines("ETHUSDT", "1h", dt(1, 0), dt(1, 2), transport=fake_klines(rows))
    assert [c.close for c in candles] == ["2005", "1990"]
    assert candles[0].ts == dt(1, 0)


def test_ingest_binance_writes_store(tmp_path) -> None:
    rows = [[1704067200000, "2000", "2010", "1990", "2005", "12"]]
    store = ParquetStore(tmp_path)
    n = ingest_binance(
        store,
        venue="hyperliquid",
        symbol="ETH",
        binance_symbol="ETHUSDT",
        interval="1h",
        start=dt(1, 0),
        end=dt(1, 1),
        transport=fake_klines(rows),
    )
    assert n == 1
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 1), "1h")
    assert src.candles("hyperliquid", "ETH")[0].close == "2005"


def test_fetch_klines_without_transport_refuses_network() -> None:
    with pytest.raises(RuntimeError, match="no transport"):
        fetch_klines("ETHUSDT", "1h", dt(1, 0), dt(1, 1))
