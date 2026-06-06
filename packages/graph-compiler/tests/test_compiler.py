"""Tests for the graph compiler."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from catalyst_graph_compiler import CompileError, CompiledGraph, compile_graph


def _repo_root() -> Path:
    for parent in Path(__file__).resolve().parents:
        if (parent / "tests" / "fixtures" / "sample_graphs.json").exists():
            return parent
    raise RuntimeError("could not locate tests/fixtures/sample_graphs.json")


SAMPLE_GRAPHS = json.loads((_repo_root() / "tests" / "fixtures" / "sample_graphs.json").read_text())


# --- All problem-statement sample graphs compile cleanly ---


@pytest.mark.parametrize("name", sorted(SAMPLE_GRAPHS))
def test_sample_graphs_compile(name: str) -> None:
    compiled = compile_graph(SAMPLE_GRAPHS[name])
    assert isinstance(compiled, CompiledGraph)
    # every action has at least one trigger
    for action in compiled.actions:
        assert action.triggers


def test_compilation_is_deterministic() -> None:
    g = SAMPLE_GRAPHS["g04_hl_spot_ladder"]
    first = compile_graph(g).model_dump()
    second = compile_graph(g).model_dump()
    assert first == second


# --- Initial vs signal-driven vs action-chained triggers ---


def test_single_swap_is_initial_action() -> None:
    compiled = compile_graph(SAMPLE_GRAPHS["g01_evm_swap_buy_eth_base"])
    assert len(compiled.actions) == 1
    assert not compiled.signals
    assert [t.type for t in compiled.actions[0].triggers] == ["initial"]


def test_signal_drives_action() -> None:
    compiled = compile_graph(SAMPLE_GRAPHS["g11_evm_swap_if_below"])
    (signal,) = compiled.signals
    assert signal.targets == ["buy-eth-on-base"]
    (action,) = compiled.actions
    assert action.triggers[0].type == "signal"
    assert action.triggers[0].source_id == "eth-below-1800"


def test_action_chains_to_action() -> None:
    compiled = compile_graph(SAMPLE_GRAPHS["g03_hl_spot_buy_then_sell"])
    by_id = {a.id: a for a in compiled.actions}
    assert by_id["buy-eth-spot"].triggers[0].type == "initial"
    sell = by_id["sell-eth-spot"]
    assert sell.triggers[0].type == "action"
    assert sell.triggers[0].source_id == "buy-eth-spot"


# --- Data requirement extraction ---


def test_evm_swap_requires_candles_and_gas() -> None:
    reqs = compile_graph(SAMPLE_GRAPHS["g01_evm_swap_buy_eth_base"]).data_requirements
    assert {(c.venue, c.symbol) for c in reqs.candles} == {("base", "ETH")}
    assert {g.chain for g in reqs.gas} == {"base"}
    assert reqs.funding == []
    assert reqs.yields == []


def test_perp_requires_candles_and_funding_no_gas() -> None:
    reqs = compile_graph(SAMPLE_GRAPHS["g05_hl_perp_open_long"]).data_requirements
    assert {(c.venue, c.symbol) for c in reqs.candles} == {("hyperliquid", "ETH")}
    assert {(f.venue, f.symbol) for f in reqs.funding} == {("hyperliquid", "ETH")}
    assert reqs.gas == []  # hyperliquid actions carry no EVM gas


def test_yield_requires_yield_source_and_gas() -> None:
    reqs = compile_graph(SAMPLE_GRAPHS["g08_evm_yield_deposit"]).data_requirements
    assert len(reqs.yields) == 1
    y = reqs.yields[0]
    assert (y.protocol, y.asset, y.chain, y.pool) == ("aave", "USDC", "base", "usdc")
    assert {g.chain for g in reqs.gas} == {"base"}
    assert reqs.candles == []  # USDC is a stable/quote asset, no price feed needed


def test_signal_price_feed_uses_traded_venue_when_unambiguous() -> None:
    # g12 trades ETH only on base, so the ETH signals resolve to base candles.
    reqs = compile_graph(SAMPLE_GRAPHS["g12_evm_swap_dca_ladder"]).data_requirements
    assert {(c.venue, c.symbol) for c in reqs.candles} == {("base", "ETH")}


def test_signal_only_graph_falls_back_to_default_price_venue() -> None:
    graph = {
        "nodes": [
            {
                "id": "eth-below-1800",
                "kind": "signal",
                "subtype": "price_threshold",
                "config": {"symbol": "ETH", "operator": "<", "threshold": "1800"},
                "enabled": True,
            }
        ],
        "edges": [],
    }
    reqs = compile_graph(graph).data_requirements
    assert {(c.venue, c.symbol) for c in reqs.candles} == {("hyperliquid", "ETH")}


# --- Enabled/disabled handling ---


def test_disabled_node_is_excluded_with_warning() -> None:
    graph = json.loads(json.dumps(SAMPLE_GRAPHS["g03_hl_spot_buy_then_sell"]))
    graph["nodes"][1]["enabled"] = False  # disable the sell
    compiled = compile_graph(graph)
    assert [a.id for a in compiled.actions] == ["buy-eth-spot"]
    assert any("disabled" in w for w in compiled.warnings)


def test_edge_to_disabled_node_is_dropped_with_warning() -> None:
    graph = json.loads(json.dumps(SAMPLE_GRAPHS["g03_hl_spot_buy_then_sell"]))
    graph["nodes"][1]["enabled"] = False
    compiled = compile_graph(graph)
    assert any("dropped" in w for w in compiled.warnings)


# --- Error cases with clear messages ---


def test_duplicate_node_id_errors() -> None:
    graph = {
        "nodes": [
            {
                "id": "dup",
                "kind": "action",
                "subtype": "swap",
                "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "1", "chain": "base"},
            },
            {
                "id": "dup",
                "kind": "action",
                "subtype": "swap",
                "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "1", "chain": "base"},
            },
        ],
        "edges": [],
    }
    with pytest.raises(CompileError, match="duplicate node id"):
        compile_graph(graph)


def test_edge_to_unknown_node_errors() -> None:
    graph = json.loads(json.dumps(SAMPLE_GRAPHS["g01_evm_swap_buy_eth_base"]))
    graph["edges"] = [{"from": "buy-eth-on-base", "to": "does-not-exist"}]
    with pytest.raises(CompileError, match="unknown target node"):
        compile_graph(graph)


def test_malformed_config_errors_with_node_id() -> None:
    graph = {
        "nodes": [
            {
                "id": "bad-swap",
                "kind": "action",
                "subtype": "swap",
                "config": {"from_asset": "USDC", "to_asset": "ETH", "chain": "base"},
            },  # missing amount
        ],
        "edges": [],
    }
    with pytest.raises(CompileError, match="bad-swap"):
        compile_graph(graph)


def test_empty_graph_errors() -> None:
    with pytest.raises(CompileError, match="no nodes"):
        compile_graph({"nodes": [], "edges": []})
