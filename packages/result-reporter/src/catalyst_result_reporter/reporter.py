"""Turn a raw simulation trace into a user-facing backtest result.

The reporter is pure: it reads a ``SimulationTrace`` and produces a
``BacktestResult`` matching ``backtest-result.schema.json``. It computes the
equity/drawdown curves, a trade log, a costs breakdown, and carries the resolved
policy and data-provider metadata through unchanged so results stay explainable.
"""

from __future__ import annotations

from decimal import Decimal
from typing import Any

from catalyst_contracts import (
    BacktestResult,
    Costs,
    DrawdownPoint,
    EquityPoint,
    ResultMetadata,
    SimulationTrace,
    Summary,
    Trade,
)

# Event types that represent a position change worth logging as a "trade".
_EXECUTED = "action_executed"
_REJECTED = "action_rejected"
_LIQUIDATION = "liquidation"


def _dec(value: Any) -> Decimal:
    if value is None:
        return Decimal(0)
    return Decimal(str(value))


def _s(value: Decimal) -> str:
    # Normalize to a plain decimal string (no exponent), trimming trailing zeros.
    return format(value.normalize(), "f")


def summarize(
    trace: SimulationTrace | dict,
    *,
    data_coverage: list[dict] | None = None,
    starting_value_usd: str | None = None,
) -> BacktestResult:
    """Summarize ``trace`` into a ``BacktestResult``.

    ``data_coverage`` is the provider metadata from the market data bundle
    (preserved verbatim in the result). ``starting_value_usd`` overrides the
    starting equity (otherwise the first snapshot's equity is used).
    """

    if not isinstance(trace, SimulationTrace):
        trace = SimulationTrace.model_validate(trace)

    equity_curve = [EquityPoint(ts=s.ts, equity_usd=s.equity_usd) for s in trace.snapshots]

    start_val = (
        _dec(starting_value_usd)
        if starting_value_usd is not None
        else (_dec(trace.snapshots[0].equity_usd) if trace.snapshots else Decimal(0))
    )
    final_val = _dec(trace.snapshots[-1].equity_usd) if trace.snapshots else start_val
    pnl = final_val - start_val
    return_pct = (pnl / start_val * Decimal(100)) if start_val != 0 else Decimal(0)

    drawdown_curve, max_drawdown = _drawdown(trace)
    trades = _trades(trace)
    costs = _costs(trace)

    executed = sum(1 for e in trace.events if e.type == _EXECUTED)
    rejected = sum(1 for e in trace.events if e.type == _REJECTED)

    summary = Summary(
        starting_value_usd=_s(start_val),
        final_value_usd=_s(final_val),
        pnl_usd=_s(pnl),
        return_pct=_s(return_pct),
        max_drawdown_pct=_s(max_drawdown),
        trade_count=executed,
        rejected_count=rejected,
    )

    metadata = ResultMetadata(
        policy=trace.policy,
        interval=trace.interval,
        start=trace.start,
        end=trace.end,
        data_coverage=data_coverage or [],
        warnings=list(trace.warnings),
    )

    return BacktestResult(
        summary=summary,
        equity_curve=equity_curve,
        drawdown_curve=drawdown_curve,
        trades=trades,
        final_portfolio=trace.final_portfolio,
        costs=costs,
        metadata=metadata,
    )


def _drawdown(trace: SimulationTrace) -> tuple[list[DrawdownPoint], Decimal]:
    curve: list[DrawdownPoint] = []
    peak = Decimal(0)
    max_dd = Decimal(0)
    for snap in trace.snapshots:
        equity = _dec(snap.equity_usd)
        peak = max(peak, equity)
        dd = ((equity - peak) / peak * Decimal(100)) if peak > 0 else Decimal(0)
        max_dd = min(max_dd, dd)
        curve.append(DrawdownPoint(ts=snap.ts, drawdown_pct=_s(dd)))
    return curve, max_dd


def _trades(trace: SimulationTrace) -> list[Trade]:
    trades: list[Trade] = []
    for event in trace.events:
        if event.type == _EXECUTED:
            d = event.detail or {}
            trades.append(
                Trade(
                    ts=event.ts,
                    node_id=event.node_id or "",
                    kind=str(d.get("kind", "action")),
                    venue=d.get("venue"),
                    symbol=d.get("symbol"),
                    side=d.get("side"),
                    price=_opt_str(d.get("price")),
                    amount=_opt_str(d.get("amount")),
                    value_usd=_opt_str(d.get("value_usd")),
                    fee_usd=_opt_str(d.get("fee_usd")),
                    gas_usd=_opt_str(d.get("gas_usd")),
                    status="executed",
                )
            )
        elif event.type == _REJECTED:
            trades.append(
                Trade(
                    ts=event.ts,
                    node_id=event.node_id or "",
                    kind="rejected",
                    status="rejected",
                    reason=event.reason,
                )
            )
        elif event.type == _LIQUIDATION:
            d = event.detail or {}
            trades.append(
                Trade(
                    ts=event.ts,
                    node_id=event.node_id or "",
                    kind="liquidation",
                    venue=d.get("venue"),
                    symbol=d.get("symbol"),
                    price=_opt_str(d.get("mark")),
                    status="executed",
                    reason=event.reason,
                )
            )
    return trades


def _costs(trace: SimulationTrace) -> Costs:
    fees = gas = funding = yield_ = Decimal(0)
    for event in trace.events:
        d = event.detail or {}
        if event.type == _EXECUTED:
            fees += _dec(d.get("fee_usd"))
            gas += _dec(d.get("gas_usd"))
        elif event.type == "funding_applied":
            funding += _dec(d.get("payment_usd"))
        elif event.type == "yield_accrued":
            yield_ += _dec(d.get("interest_usd"))
    return Costs(
        total_fees_usd=_s(fees),
        total_gas_usd=_s(gas),
        total_funding_usd=_s(funding),
        total_yield_usd=_s(yield_),
    )


def _opt_str(value: Any) -> str | None:
    return None if value is None else str(value)


__all__ = ["summarize"]
