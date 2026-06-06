"""Tests for the Aave-yields (DefiLlama) and EVM-gas (eth_feeHistory) sources."""

from __future__ import annotations

from datetime import UTC, datetime
from decimal import Decimal

import pytest

from catalyst_market_data import (
    ParquetSource,
    ParquetStore,
    constant_gas_series,
    fetch_aave_yields,
    fetch_recent_gas,
    ingest_aave_yields,
    ingest_constant_gas,
)


def dt(day: int, hour: int = 0) -> datetime:
    return datetime(2024, 1, day, hour, tzinfo=UTC)


# --- Aave yields via DefiLlama ---


def fake_yields(rows):
    def transport(url, params):
        return {"status": "success", "data": rows}

    return transport


def test_fetch_aave_yields_converts_percent_to_fraction() -> None:
    rows = [
        {"timestamp": "2024-01-01T00:00:00.000Z", "apy": 4.5, "tvlUsd": 1},
        {"timestamp": "2024-01-02T00:00:00.000Z", "apy": 5.0, "tvlUsd": 1},
    ]
    points = fetch_aave_yields("pool-uuid", dt(1), dt(3), transport=fake_yields(rows))
    assert [p.apr for p in points] == ["0.045", "0.05"]
    assert points[0].ts == dt(1)


def test_fetch_aave_yields_windows_and_validates() -> None:
    rows = [
        {"timestamp": "2023-12-31T00:00:00.000Z", "apy": 1.0},  # before window
        {"timestamp": "2024-01-01T00:00:00.000Z", "apy": 4.5},
    ]
    points = fetch_aave_yields("p", dt(1), dt(2), transport=fake_yields(rows))
    assert len(points) == 1

    def bad_transport(url, params):
        return {"oops": True}

    with pytest.raises(RuntimeError, match="unexpected DefiLlama"):
        fetch_aave_yields("p", dt(1), dt(2), transport=bad_transport)


def test_ingest_aave_yields_round_trips(tmp_path) -> None:
    rows = [{"timestamp": "2024-01-01T00:00:00.000Z", "apy": 4.5}]
    store = ParquetStore(tmp_path)
    n = ingest_aave_yields(
        store,
        asset="USDC",
        chain="base",
        pool="usdc",
        pool_id="p",
        start=dt(1),
        end=dt(2),
        transport=fake_yields(rows),
    )
    assert n == 1
    src = ParquetSource(tmp_path, dt(1), dt(2), "1h")
    assert src.yields("aave", "USDC", "base", "usdc")[0].apr == "0.045"


# --- EVM gas via eth_feeHistory ---


def fake_rpc(base_fees_hex, oldest_hex, oldest_ts_hex):
    def transport(url, method, params):
        if method == "eth_feeHistory":
            return {"baseFeePerGas": base_fees_hex, "oldestBlock": oldest_hex, "gasUsedRatio": []}
        if method == "eth_getBlockByNumber":
            return {"timestamp": oldest_ts_hex}
        raise AssertionError(method)

    return transport


def test_fetch_recent_gas_computes_usd() -> None:
    # base fee 1 gwei = 1e9 wei; 2 blocks (+1 forecast). gas_units 100000, ETH $2000.
    one_gwei = hex(10**9)
    transport = fake_rpc([one_gwei, one_gwei, one_gwei], hex(100), hex(1_704_067_200))
    points = fetch_recent_gas(
        "http://rpc",
        block_count=2,
        gas_units=100_000,
        eth_price_usd="2000",
        transport=transport,
        block_time_seconds=2,
    )
    assert len(points) == 2  # forecast entry dropped
    # 1e9 wei * 1e5 / 1e18 * 2000 = 0.0002 ETH * 2000 = 0.2 USD
    assert Decimal(points[0].gas_usd) == Decimal("0.2")
    assert points[0].ts == datetime.fromtimestamp(1_704_067_200, tz=UTC)
    assert points[1].ts == datetime.fromtimestamp(1_704_067_202, tz=UTC)


def test_constant_gas_series_and_ingest(tmp_path) -> None:
    series = constant_gas_series(dt(1, 0), dt(1, 3), "1h", "0.02")
    assert len(series) == 4  # 00,01,02,03
    assert all(p.gas_usd == "0.02" for p in series)

    store = ParquetStore(tmp_path)
    n = ingest_constant_gas(
        store, chain="base", start=dt(1, 0), end=dt(1, 3), interval="1h", gas_usd="0.02"
    )
    assert n == 4
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 3), "1h")
    assert [p.gas_usd for p in src.gas("base")] == ["0.02"] * 4
