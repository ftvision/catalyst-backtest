"""Pydantic models parse the example payloads and round-trip losslessly."""

from __future__ import annotations

import json

import pytest
from pydantic import ValidationError

from catalyst_contracts import (
    BacktestRequest,
    BacktestResult,
    Graph,
    MarketDataBundle,
    SimulationPolicy,
    SimulationTrace,
    SwapConfig,
)
from catalyst_contracts.schemas import schemas_dir

EXAMPLES_DIR = schemas_dir() / "examples"


def _load(name: str) -> dict:
    return json.loads((EXAMPLES_DIR / name).read_text())


@pytest.mark.parametrize(
    "filename,model",
    [
        ("graph.swap.json", Graph),
        ("graph.perp-signal.json", Graph),
        ("graph.yield.json", Graph),
        ("graph.limit-order.json", Graph),
        ("backtest-request.json", BacktestRequest),
        ("simulation-policy.strict_v1.json", SimulationPolicy),
        ("market-data-bundle.json", MarketDataBundle),
        ("simulation-trace.json", SimulationTrace),
        ("backtest-result.json", BacktestResult),
    ],
)
def test_model_parses_example(filename, model) -> None:
    model.model_validate(_load(filename))


def test_edge_from_alias_round_trips() -> None:
    graph = Graph.model_validate(_load("graph.perp-signal.json"))
    assert graph.edges[0].from_ == "eth-below-1800"
    dumped = graph.model_dump(by_alias=True, exclude_none=True)
    assert dumped["edges"][0]["from"] == "eth-below-1800"


def test_policy_yield_alias_round_trips() -> None:
    policy = SimulationPolicy.model_validate(_load("simulation-policy.strict_v1.json"))
    assert policy.yield_.accrual == "simple_apr"
    dumped = policy.model_dump(by_alias=True)
    assert "yield" in dumped and dumped["yield"]["accrual"] == "simple_apr"


def test_typed_swap_config_parses_node_config() -> None:
    graph = Graph.model_validate(_load("graph.swap.json"))
    cfg = SwapConfig.model_validate(graph.nodes[0].config)
    assert cfg.from_asset == "USDC"
    assert cfg.to_asset == "ETH"
    assert cfg.amount == "100"


def test_extra_fields_rejected_on_strict_models() -> None:
    payload = _load("simulation-policy.strict_v1.json")
    payload["unexpected"] = True
    with pytest.raises(ValidationError):
        SimulationPolicy.model_validate(payload)
