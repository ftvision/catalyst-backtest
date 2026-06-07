"""Offline tests for the Dune ingester (fake transport, no network)."""

from __future__ import annotations

from datetime import UTC, datetime

import pytest

from catalyst_market_data_core import ParquetSource, ParquetStore
from catalyst_market_data_dune import DuneClient, fetch_candles, fetch_gas, ingest_gas, parse_ts


class FakeTransport:
    """Simulates Dune execute -> status -> results."""

    def __init__(self, rows, *, states=None) -> None:
        self.rows = rows
        self.states = list(states or ["QUERY_STATE_COMPLETED"])
        self.calls: list[tuple] = []

    def __call__(self, method, url, *, headers=None, params=None, json=None):
        self.calls.append((method, url, headers, json))
        if method == "POST" and url.endswith("/execute"):
            return {"execution_id": "exec-1"}
        if url.endswith("/status"):
            return {"state": self.states.pop(0) if len(self.states) > 1 else self.states[0]}
        if url.endswith("/results"):
            return {"result": {"rows": self.rows}}
        raise AssertionError(f"unexpected call {method} {url}")


def client(rows, **kw) -> tuple[DuneClient, FakeTransport]:
    t = FakeTransport(rows, **kw)
    return DuneClient("key-123", t, sleep=lambda _s: None, poll_interval=0), t


def dt(day: int, hour: int = 0) -> datetime:
    return datetime(2024, 1, day, hour, tzinfo=UTC)


def test_run_query_executes_polls_and_passes_key_and_params() -> None:
    c, t = client([{"ts": "2024-01-01 00:00:00.000 UTC", "gas_usd": "0.5"}])
    rows = c.run_query(1234, {"start": "a", "end": "b"})
    assert rows[0]["gas_usd"] == "0.5"
    # api key header on every call; params forwarded on execute
    assert all(call[2] == {"X-Dune-API-Key": "key-123"} for call in t.calls)
    execute = t.calls[0]
    assert execute[3] == {"query_parameters": {"start": "a", "end": "b"}}


def test_run_query_waits_for_completion() -> None:
    c, _ = client(
        [{"ts": "2024-01-01T00:00:00Z", "gas_usd": "1"}],
        states=["QUERY_STATE_EXECUTING", "QUERY_STATE_COMPLETED"],
    )
    assert len(c.run_query(1)) == 1


def test_failed_query_raises() -> None:
    c, _ = client([], states=["QUERY_STATE_FAILED"])
    with pytest.raises(RuntimeError, match="QUERY_STATE_FAILED"):
        c.run_query(1)


def test_parse_ts_handles_dune_and_iso_formats() -> None:
    assert parse_ts("2024-01-01 00:00:00.000 UTC") == dt(1, 0)
    assert parse_ts("2024-01-01T05:00:00Z") == dt(1, 5)
    assert parse_ts(dt(2, 0)) == dt(2, 0)


def test_fetch_gas_maps_rows() -> None:
    c, _ = client(
        [
            {"ts": "2024-01-01 00:00:00.000 UTC", "gas_usd": 0.5},
            {"ts": "2024-01-01 01:00:00.000 UTC", "gas_usd": 0.6},
        ]
    )
    points = fetch_gas(c, 1, start=dt(1, 0), end=dt(1, 2))
    assert [p.gas_usd for p in points] == ["0.5", "0.6"]
    assert points[0].ts == dt(1, 0)


def test_fetch_candles_maps_and_handles_missing_volume() -> None:
    c, _ = client([{"ts": "2024-01-01T00:00:00Z", "open": 2000, "high": 2010, "low": 1990, "close": 2005}])
    candles = fetch_candles(c, 1, start=dt(1, 0), end=dt(1, 2))
    assert candles[0].close == "2005"
    assert candles[0].volume is None


def test_ingest_gas_writes_store(tmp_path) -> None:
    c, _ = client([{"ts": "2024-01-01 00:00:00.000 UTC", "gas_usd": "0.42"}])
    store = ParquetStore(tmp_path)
    n = ingest_gas(store, c, chain="ethereum", query_id=1, start=dt(1, 0), end=dt(1, 2))
    assert n == 1
    src = ParquetSource(tmp_path, dt(1, 0), dt(1, 2), "1h")
    assert src.gas("ethereum")[0].gas_usd == "0.42"


def test_no_transport_refuses_network() -> None:
    c = DuneClient("key")  # default transport is network_disabled
    with pytest.raises(RuntimeError, match="no transport configured"):
        c.run_query(1)
