"""Tests for the market data package (all offline / network-free)."""

from __future__ import annotations

import json
from datetime import UTC, datetime
from pathlib import Path

import pytest

from catalyst_graph_compiler import compile_graph
from catalyst_market_data import (
    BundleCache,
    CompositeSource,
    FixtureSource,
    HyperliquidSource,
    MissingDataError,
    bundle_key,
    build_bundle,
)
from catalyst_market_data.live import CallableGasSource, CallableYieldSource


def _repo_root() -> Path:
    for parent in Path(__file__).resolve().parents:
        if (parent / "tests" / "fixtures" / "sample_graphs.json").exists():
            return parent
    raise RuntimeError("repo root not found")


ROOT = _repo_root()
SAMPLE_GRAPHS = json.loads((ROOT / "tests" / "fixtures" / "sample_graphs.json").read_text())
FIXTURE_BUNDLE = ROOT / "tests" / "fixtures" / "market_data" / "eth_2h.json"

START = datetime(2024, 1, 1, 0, 0, tzinfo=UTC)
END = datetime(2024, 1, 1, 2, 0, tzinfo=UTC)


def fixture_source() -> FixtureSource:
    return FixtureSource.from_file(FIXTURE_BUNDLE)


def build(name: str, source=None, **kw):
    compiled = compile_graph(SAMPLE_GRAPHS[name])
    return build_bundle(
        compiled,
        start=START,
        end=END,
        interval="1h",
        source=source or fixture_source(),
        **kw,
    )


# --- FixtureSource lookups ---


def test_fixture_source_returns_known_series() -> None:
    src = fixture_source()
    assert len(src.candles("base", "ETH")) == 2
    assert len(src.funding("hyperliquid", "ETH")) == 2
    assert len(src.gas("base")) == 2
    assert len(src.yields("aave", "USDC", "base", "usdc")) == 2
    assert src.candles("base", "DOGE") == []


# --- Bundle assembly per graph family ---


def test_evm_swap_bundle_has_candles_and_gas() -> None:
    bundle = build("g01_evm_swap_buy_eth_base")
    assert {(s.venue, s.symbol) for s in bundle.candles} == {("base", "ETH")}
    assert {s.chain for s in bundle.gas} == {"base"}
    assert bundle.warnings == []
    assert {p.kind for p in bundle.providers} == {"candles", "gas"}
    assert all(p.coverage.complete for p in bundle.providers)


def test_perp_bundle_has_candles_and_funding() -> None:
    bundle = build("g05_hl_perp_open_long")
    assert {(s.venue, s.symbol) for s in bundle.candles} == {("hyperliquid", "ETH")}
    assert {(s.venue, s.symbol) for s in bundle.funding} == {("hyperliquid", "ETH")}
    assert bundle.warnings == []


def test_yield_bundle_has_yield_series() -> None:
    bundle = build("g08_evm_yield_deposit")
    assert len(bundle.yields) == 1
    assert bundle.yields[0].protocol == "aave"
    assert bundle.warnings == []


def test_bundle_is_serializable_and_matches_contract() -> None:
    from catalyst_contracts import MarketDataBundle

    bundle = build("g01_evm_swap_buy_eth_base")
    dumped = bundle.model_dump_json()
    MarketDataBundle.model_validate_json(dumped)  # round-trips through the contract


# --- Missing data behavior ---


def test_missing_data_warns_by_default() -> None:
    empty = FixtureSource.from_dict(
        {"interval": "1h", "start": "2024-01-01T00:00:00Z", "end": "2024-01-01T02:00:00Z"}
    )
    bundle = build("g01_evm_swap_buy_eth_base", source=empty)
    assert any("candles for ETH on base" in w for w in bundle.warnings)
    assert not all(p.coverage.complete for p in bundle.providers)


def test_missing_data_can_fail() -> None:
    empty = FixtureSource.from_dict(
        {"interval": "1h", "start": "2024-01-01T00:00:00Z", "end": "2024-01-01T02:00:00Z"}
    )
    with pytest.raises(MissingDataError):
        build("g01_evm_swap_buy_eth_base", source=empty, missing="fail")


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
    bundle = build("g01_evm_swap_buy_eth_base")
    cache = BundleCache(tmp_path)
    key = bundle_key(
        start=START,
        end=END,
        interval="1h",
        requirements=compile_graph(
            SAMPLE_GRAPHS["g01_evm_swap_buy_eth_base"]
        ).data_requirements.model_dump(),
    )
    assert cache.get(key) is None
    cache.put(key, bundle)
    restored = cache.get(key)
    assert restored is not None
    assert restored.model_dump() == bundle.model_dump()
