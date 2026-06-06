"""Tests for the market data package (all offline / network-free).

The run path (compiling a graph, assembling a bundle for the engine) is Rust now;
these tests cover what Python still owns: source lookups, the live fetchers, and
the bundle cache. See `test_parquet_store.py` for the Parquet store + ingesters.
"""

from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path

import pytest

from catalyst_contracts import (
    Candle,
    CandleSeries,
    MarketDataBundle,
    Provider,
)
from catalyst_contracts.market_data import Coverage
from catalyst_market_data import (
    BundleCache,
    CompositeSource,
    FixtureSource,
    HyperliquidSource,
    bundle_key,
)
from catalyst_market_data.live import CallableGasSource, CallableYieldSource


def _repo_root() -> Path:
    for parent in Path(__file__).resolve().parents:
        if (parent / "tests" / "fixtures" / "market_data" / "eth_2h.json").exists():
            return parent
    raise RuntimeError("repo root not found")


ROOT = _repo_root()
FIXTURE_BUNDLE = ROOT / "tests" / "fixtures" / "market_data" / "eth_2h.json"

START = datetime(2024, 1, 1, 0, 0, tzinfo=UTC)
END = datetime(2024, 1, 1, 2, 0, tzinfo=UTC)


def fixture_source() -> FixtureSource:
    return FixtureSource.from_file(FIXTURE_BUNDLE)


def sample_bundle() -> MarketDataBundle:
    """A tiny hand-built bundle for cache/serialization tests."""
    series = CandleSeries(
        venue="base",
        symbol="ETH",
        points=[Candle(ts=START, open="2000", high="2000", low="2000", close="2000", volume="1")],
    )
    return MarketDataBundle(
        interval="1h",
        start=START,
        end=END,
        candles=[series],
        funding=[],
        gas=[],
        yields=[],
        providers=[
            Provider(
                name="fixture",
                kind="candles",
                coverage=Coverage(start="2024-01-01T00:00:00Z", end="2024-01-01T02:00:00Z", complete=True),
            )
        ],
        warnings=[],
    )


# --- FixtureSource lookups ---


def test_fixture_source_returns_known_series() -> None:
    src = fixture_source()
    assert len(src.candles("base", "ETH")) == 2
    assert len(src.funding("hyperliquid", "ETH")) == 2
    assert len(src.gas("base")) == 2
    assert len(src.yields("aave", "USDC", "base", "usdc")) == 2
    assert src.candles("base", "DOGE") == []


def test_sample_bundle_round_trips_through_contract() -> None:
    dumped = sample_bundle().model_dump_json()
    MarketDataBundle.model_validate_json(dumped)


# --- Hyperliquid source request building + parsing (offline fake transport) ---


class FakeTransport:
    def __init__(self, response) -> None:
        self.response = response
        self.calls: list[tuple[str, dict]] = []

    def post(self, url: str, body: dict):
        self.calls.append((url, body))
        return self.response


def test_hyperliquid_candles_request_and_parse() -> None:
    rows = [
        {"t": 1704067200000, "o": "2000", "h": "2010", "l": "1990", "c": "2005", "v": "12"},
        {"t": 1704070800000, "o": "2005", "h": "2050", "l": "1780", "c": "1800", "v": "19"},
    ]
    transport = FakeTransport(rows)
    src = HyperliquidSource(START, END, "1h", transport=transport)
    candles = src.candles("hyperliquid", "ETH")
    assert [c.close for c in candles] == ["2005", "1800"]
    assert candles[0].ts == START
    # request was built for the right coin/interval/range
    url, body = transport.calls[0]
    assert body["type"] == "candleSnapshot"
    assert body["req"]["coin"] == "ETH"
    assert body["req"]["interval"] == "1h"
    assert body["req"]["startTime"] == 1704067200000


def test_hyperliquid_funding_parse() -> None:
    rows = [{"coin": "ETH", "fundingRate": "0.0001", "time": 1704067200000}]
    src = HyperliquidSource(START, END, "1h", transport=FakeTransport(rows))
    funding = src.funding("hyperliquid", "ETH")
    assert funding[0].rate == "0.0001"
    assert funding[0].ts == START


def test_hyperliquid_default_transport_refuses_network() -> None:
    src = HyperliquidSource(START, END, "1h")
    with pytest.raises(RuntimeError, match="network is disabled"):
        src.candles("hyperliquid", "ETH")


# --- Composite routing + injectable gas/yield sources ---


def test_composite_routes_each_kind() -> None:
    gas = CallableGasSource(lambda chain: [("2024-01-01T00:00:00Z", "0.02")])
    yields = CallableYieldSource(
        lambda protocol, asset, chain, pool: [("2024-01-01T00:00:00Z", "0.05")]
    )
    composite = CompositeSource(candles=fixture_source(), gas=gas, yields=yields)
    assert composite.candles("base", "ETH")  # from fixture
    assert composite.gas("base")[0].gas_usd == "0.02"
    assert composite.yields("aave", "USDC", "base", "usdc")[0].apr == "0.05"


# --- Cache ---


def test_bundle_cache_round_trip(tmp_path) -> None:
    bundle = sample_bundle()
    cache = BundleCache(tmp_path)
    key = bundle_key(
        start=START,
        end=END,
        interval="1h",
        requirements={"candles": [{"venue": "base", "symbol": "ETH"}]},
    )
    assert cache.get(key) is None
    cache.put(key, bundle)
    restored = cache.get(key)
    assert restored is not None
    assert restored.model_dump() == bundle.model_dump()
