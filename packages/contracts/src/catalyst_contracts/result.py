"""Backtest result contract models (backtest-result.schema.json)."""

from __future__ import annotations

from datetime import datetime
from typing import Any, Literal

from pydantic import Field

from ._base import Decimal, StrictModel
from .policy import SimulationPolicy
from .trace import Portfolio


class Summary(StrictModel):
    starting_value_usd: Decimal
    final_value_usd: Decimal
    pnl_usd: Decimal
    return_pct: Decimal
    max_drawdown_pct: Decimal | None = None
    trade_count: int | None = None
    rejected_count: int | None = None


class EquityPoint(StrictModel):
    ts: datetime
    equity_usd: Decimal


class DrawdownPoint(StrictModel):
    ts: datetime
    drawdown_pct: Decimal


class Trade(StrictModel):
    ts: datetime
    node_id: str
    kind: str
    venue: str | None = None
    symbol: str | None = None
    side: str | None = None
    price: Decimal | None = None
    amount: Decimal | None = None
    value_usd: Decimal | None = None
    fee_usd: Decimal | None = None
    gas_usd: Decimal | None = None
    status: Literal["executed", "rejected"] | None = None
    reason: str | None = None


class Costs(StrictModel):
    total_fees_usd: Decimal | None = None
    total_gas_usd: Decimal | None = None
    total_funding_usd: Decimal | None = None
    total_yield_usd: Decimal | None = None


class ResultMetadata(StrictModel):
    policy: SimulationPolicy
    interval: str | None = None
    start: datetime | None = None
    end: datetime | None = None
    effective_start: datetime | None = None
    effective_end: datetime | None = None
    data_coverage: list[dict[str, Any]] = Field(default_factory=list)
    warnings: list[str] = Field(default_factory=list)


class BacktestResult(StrictModel):
    schema_version: str = "catalyst.backtest.result.v1"
    summary: Summary
    equity_curve: list[EquityPoint] = Field(default_factory=list)
    drawdown_curve: list[DrawdownPoint] = Field(default_factory=list)
    trades: list[Trade] = Field(default_factory=list)
    final_portfolio: Portfolio | None = None
    costs: Costs | None = None
    metadata: ResultMetadata


__all__ = [
    "BacktestResult",
    "Summary",
    "EquityPoint",
    "DrawdownPoint",
    "Trade",
    "Costs",
    "ResultMetadata",
]
