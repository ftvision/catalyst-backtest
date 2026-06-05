"""Tests for the result reporter."""

from __future__ import annotations

import pytest

from catalyst_contracts import validate
from catalyst_result_reporter import summarize

POLICY = {"schema_version": "catalyst.backtest.policy.v1", "profile": "strict_v1"}


def base_trace(**overrides) -> dict:
    trace = {
        "schema_version": "catalyst.backtest.trace.v1",
        "policy": POLICY,
        "interval": "1h",
        "start": "2024-01-01T00:00:00Z",
        "end": "2024-01-01T02:00:00Z",
        "snapshots": [],
        "events": [],
        "final_portfolio": {"balances": {}, "perp_positions": [], "yield_positions": []},
        "warnings": [],
        "errors": [],
    }
    trace.update(overrides)
    return trace


def snap(ts: str, equity: str) -> dict:
    return {"ts": ts, "equity_usd": equity}


# --- Schema conformance + policy/provider metadata ---


def test_result_matches_schema_and_preserves_policy_and_providers() -> None:
    providers = [{"name": "fixture", "kind": "candles"}]
    trace = base_trace(
        snapshots=[snap("2024-01-01T00:00:00Z", "2000"), snap("2024-01-01T01:00:00Z", "2100")],
    )
    result = summarize(trace, data_coverage=providers)
    dumped = result.model_dump(by_alias=True, exclude_none=True, mode="json")
    validate(dumped, "backtest-result")
    assert result.metadata.policy.profile == "strict_v1"
    assert result.metadata.data_coverage == providers


# --- Summary math: pnl, return, drawdown ---


def test_summary_and_drawdown_math() -> None:
    trace = base_trace(
        snapshots=[
            snap("2024-01-01T00:00:00Z", "1000"),
            snap("2024-01-01T01:00:00Z", "1200"),  # peak
            snap("2024-01-01T02:00:00Z", "900"),  # trough -> -25% from peak
        ]
    )
    result = summarize(trace)
    assert result.summary.starting_value_usd == "1000"
    assert result.summary.final_value_usd == "900"
    assert result.summary.pnl_usd == "-100"
    assert result.summary.return_pct == "-10"
    assert result.summary.max_drawdown_pct == "-25"
    assert len(result.drawdown_curve) == 3


# --- Empty run ---


def test_empty_run_is_zeroed() -> None:
    result = summarize(base_trace())
    assert result.summary.starting_value_usd == "0"
    assert result.summary.final_value_usd == "0"
    assert result.summary.pnl_usd == "0"
    assert result.summary.return_pct == "0"
    assert result.equity_curve == []
    assert result.trades == []
    validate(result.model_dump(by_alias=True, exclude_none=True, mode="json"), "backtest-result")


# --- Trades: executed, rejected, liquidation ---


def test_executed_and_rejected_trades_and_costs() -> None:
    trace = base_trace(
        snapshots=[snap("2024-01-01T00:00:00Z", "1000")],
        events=[
            {
                "ts": "2024-01-01T00:00:00Z",
                "type": "action_executed",
                "node_id": "buy",
                "detail": {
                    "kind": "swap",
                    "venue": "base",
                    "symbol": "ETH",
                    "side": "buy",
                    "price": "2002",
                    "amount": "0.05",
                    "value_usd": "100",
                    "fee_usd": "0.05",
                    "gas_usd": "0.02",
                },
            },
            {
                "ts": "2024-01-01T01:00:00Z",
                "type": "action_rejected",
                "node_id": "sell",
                "reason": "insufficient balance",
            },
        ],
    )
    result = summarize(trace)
    assert result.summary.trade_count == 1
    assert result.summary.rejected_count == 1
    kinds = {(t.node_id, t.status) for t in result.trades}
    assert ("buy", "executed") in kinds
    assert ("sell", "rejected") in kinds
    assert result.costs.total_fees_usd == "0.05"
    assert result.costs.total_gas_usd == "0.02"


def test_liquidation_event_is_logged() -> None:
    trace = base_trace(
        snapshots=[snap("2024-01-01T00:00:00Z", "500")],
        events=[
            {
                "ts": "2024-01-01T00:00:00Z",
                "type": "liquidation",
                "reason": "hyperliquid ETH position liquidated",
                "detail": {
                    "venue": "hyperliquid",
                    "symbol": "ETH",
                    "mark": "1500",
                    "margin_lost_usd": "100",
                },
            }
        ],
    )
    result = summarize(trace)
    liq = [t for t in result.trades if t.kind == "liquidation"]
    assert len(liq) == 1
    assert liq[0].symbol == "ETH"
    assert liq[0].reason is not None


def test_funding_and_yield_costs_are_summed() -> None:
    trace = base_trace(
        snapshots=[snap("2024-01-01T00:00:00Z", "1000")],
        events=[
            {
                "ts": "2024-01-01T00:00:00Z",
                "type": "funding_applied",
                "detail": {"payment_usd": "1.5"},
            },
            {
                "ts": "2024-01-01T01:00:00Z",
                "type": "yield_accrued",
                "detail": {"interest_usd": "0.3"},
            },
        ],
    )
    result = summarize(trace)
    assert result.costs.total_funding_usd == "1.5"
    assert result.costs.total_yield_usd == "0.3"


# --- Multi-venue portfolio is preserved ---


def test_multi_venue_final_portfolio_preserved() -> None:
    trace = base_trace(
        snapshots=[snap("2024-01-01T00:00:00Z", "2000")],
        final_portfolio={
            "balances": {"base": {"USDC": "900", "ETH": "0.05"}, "hyperliquid": {"USDC": "1000"}},
            "perp_positions": [],
            "yield_positions": [],
        },
    )
    result = summarize(trace)
    assert set(result.final_portfolio.balances.keys()) == {"base", "hyperliquid"}
    assert result.final_portfolio.balances["base"]["ETH"] == "0.05"
    validate(result.model_dump(by_alias=True, exclude_none=True, mode="json"), "backtest-result")


@pytest.mark.parametrize("starting", ["5000", None])
def test_starting_value_override(starting) -> None:
    trace = base_trace(snapshots=[snap("2024-01-01T00:00:00Z", "1000")])
    result = summarize(trace, starting_value_usd=starting)
    expected = starting if starting is not None else "1000"
    assert result.summary.starting_value_usd == expected
