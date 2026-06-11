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


def test_effective_window_round_trips_on_trace_and_result() -> None:
    """#167: requested vs effective window. A run whose data starts late or ends
    early reports the actual tick span alongside the requested [start, end]."""
    trace_payload = _load("simulation-trace.json")
    trace_payload["effective_start"] = "2024-01-01T01:00:00Z"
    trace_payload["effective_end"] = trace_payload["end"]
    trace = SimulationTrace.model_validate(trace_payload)
    assert trace.effective_start.isoformat().startswith("2024-01-01T01:00:00")
    dumped = trace.model_dump(mode="json", by_alias=True, exclude_none=True)
    assert dumped["effective_start"] == "2024-01-01T01:00:00Z"

    result_payload = _load("backtest-result.json")
    result_payload["metadata"]["effective_start"] = "2024-01-01T01:00:00Z"
    result_payload["metadata"]["effective_end"] = result_payload["metadata"]["end"]
    result = BacktestResult.model_validate(result_payload)
    assert result.metadata.effective_start is not None
    # The fields are optional: omitting them still parses (old payloads).
    assert SimulationTrace.model_validate(_load("simulation-trace.json")).effective_start is None


def test_amm_price_impact_slippage_accepted_for_dex_swaps() -> None:
    """Real-world fidelity: an on-chain AMM swap's slippage IS price impact from
    pool reserves, so `amm_price_impact` is the realistic execution model for a
    DEX (e.g. a Base ETH swap). The Rust engine implements it (#40); the Python
    policy contract must accept it so a DEX strategy can select it via API/CLI."""
    policy = SimulationPolicy.model_validate(
        {
            "profile": "research_v1",
            "fills": {"slippage": {"model": "amm_price_impact", "bps": "0"}},
        }
    )
    assert policy.fills.slippage.model == "amm_price_impact"


def test_amm_price_impact_validates_against_schema() -> None:
    """The schema (cross-language source of truth) must also allow the model the
    Rust engine supports, or the policy is valid in Rust but rejected at the
    contract boundary."""
    from catalyst_contracts import validate

    payload = {
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "research_v1",
        "fills": {"slippage": {"model": "amm_price_impact", "bps": "0"}},
    }
    validate(payload, "simulation-policy")  # raises on failure
