"""Core store + transport tests (independent of any vendor)."""

from __future__ import annotations

from datetime import UTC, datetime

import pytest

from catalyst_contracts import Candle
from catalyst_contracts.market_data import GasPoint
from catalyst_market_data_core import ParquetSource, ParquetStore, http_transport, network_disabled


def dt(day: int, hour: int = 0) -> datetime:
    return datetime(2024, 1, day, hour, tzinfo=UTC)


def candle(day: int, hour: int, close: str) -> Candle:
    return Candle(ts=dt(day, hour), open=close, high=close, low=close, close=close, volume="1")


def test_candles_round_trip_and_window(tmp_path) -> None:
    store = ParquetStore(tmp_path)
    store.write_candles("base", "ETH", "1h", [candle(1, 0, "2000"), candle(2, 0, "2100")])
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h")
    candles = src.candles("base", "ETH")
    assert [c.close for c in candles] == ["2000"]  # day 2 pruned by the window


def test_incremental_write_merges_by_ts(tmp_path) -> None:
    store = ParquetStore(tmp_path)
    store.write_candles("base", "ETH", "1h", [candle(1, 0, "2000")])
    store.write_candles("base", "ETH", "1h", [candle(1, 0, "1999")])  # overwrite ts 0
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h")
    assert [c.close for c in src.candles("base", "ETH")] == ["1999"]


def test_gas_round_trip(tmp_path) -> None:
    store = ParquetStore(tmp_path)
    store.write_gas("base", [GasPoint(ts=dt(1, 0), gas_usd="0.02")])
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h")
    assert src.gas("base")[0].gas_usd == "0.02"


def test_network_disabled_transport_refuses() -> None:
    with pytest.raises(RuntimeError, match="no transport configured"):
        network_disabled("GET", "https://example.com")


def test_http_transport_is_constructible() -> None:
    # building the transport must not require network (httpx imported lazily inside)
    assert callable(http_transport())
