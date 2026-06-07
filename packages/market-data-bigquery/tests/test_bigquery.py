"""Offline tests for the BigQuery ingester (fake runner, no GCP)."""

from __future__ import annotations

from datetime import UTC, datetime
from decimal import Decimal

import pytest

from catalyst_market_data_bigquery import fetch_candles, fetch_gas, gas_sql, ingest_gas
from catalyst_market_data_bigquery.runner import network_disabled
from catalyst_market_data_core import ParquetSource, ParquetStore


class FakeRunner:
    def __init__(self, rows) -> None:
        self.rows = rows
        self.sql: str | None = None

    def __call__(self, sql: str) -> list[dict]:
        self.sql = sql
        return self.rows


def dt(day: int, hour: int = 0) -> datetime:
    return datetime(2024, 1, day, hour, tzinfo=UTC)


def test_gas_sql_targets_blocks_table_and_window() -> None:
    sql = gas_sql(dt(1, 0), dt(2, 0))
    assert "bigquery-public-data.crypto_ethereum.blocks" in sql
    assert "base_fee_per_gas" in sql
    assert "TIMESTAMP('2024-01-01 00:00:00')" in sql
    assert "GROUP BY ts" in sql


def test_fetch_gas_converts_base_fee_to_usd() -> None:
    # 20 gwei * 100k units = 0.002 ETH; * $2500 = $5.00
    runner = FakeRunner([{"ts": dt(1, 0), "base_fee_wei": 20_000_000_000}])
    points = fetch_gas(runner, start=dt(1, 0), end=dt(1, 2), gas_units=100_000, eth_price_usd="2500")
    assert Decimal(points[0].gas_usd) == Decimal("5")
    assert points[0].ts == dt(1, 0)
    # the built-in gas SQL was used
    assert "crypto_ethereum.blocks" in runner.sql


def test_fetch_gas_accepts_sql_override() -> None:
    runner = FakeRunner([{"ts": "2024-01-01T00:00:00Z", "base_fee_wei": 1_000_000_000}])
    fetch_gas(
        runner, start=dt(1, 0), end=dt(1, 2), gas_units=1, eth_price_usd="1",
        sql="SELECT custom",
    )
    assert runner.sql == "SELECT custom"


def test_ingest_gas_writes_store(tmp_path) -> None:
    runner = FakeRunner([{"ts": dt(1, 0), "base_fee_wei": 10_000_000_000}])
    store = ParquetStore(tmp_path)
    n = ingest_gas(
        store, runner, chain="ethereum", start=dt(1, 0), end=dt(1, 2),
        gas_units=100_000, eth_price_usd="2000",
    )
    assert n == 1
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h")
    assert Decimal(src.gas("ethereum")[0].gas_usd) == Decimal("2")  # 0.001 ETH * 2000


def test_fetch_candles_byo_sql_maps_and_handles_missing_volume() -> None:
    runner = FakeRunner(
        [{"ts": dt(1, 0), "open": 2000, "high": 2010, "low": 1990, "close": 2005}]
    )
    candles = fetch_candles(runner, "SELECT ...")
    assert candles[0].close == "2005"
    assert candles[0].volume is None
    assert runner.sql == "SELECT ..."


def test_network_disabled_runner_refuses() -> None:
    with pytest.raises(RuntimeError, match="no query runner configured"):
        network_disabled("SELECT 1")
