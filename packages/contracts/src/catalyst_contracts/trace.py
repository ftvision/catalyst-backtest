"""Simulation trace contract models (simulation-trace.schema.json).

The trace is the raw, deterministic output of the Rust simulation engine. The
``Portfolio`` / ``PerpPosition`` / ``YieldPosition`` models here are also reused
by the backtest result contract.
"""

from __future__ import annotations

from datetime import datetime
from typing import Any, Literal

from pydantic import Field

from ._base import Decimal, OpenModel, StrictModel
from .policy import SimulationPolicy
from .request import Interval

EventType = Literal[
    "signal_fired",
    "action_executed",
    "action_rejected",
    "funding_applied",
    "funding_shortfall",
    "yield_accrued",
    "liquidation",
    "gas_charged",
    "fee_charged",
]


class PerpPosition(StrictModel):
    venue: str
    symbol: str
    side: Literal["long", "short"]
    size: Decimal
    entry_price: Decimal
    leverage: Decimal | None = None
    margin_usd: Decimal | None = None
    liquidation_price: Decimal | None = None


class YieldPosition(StrictModel):
    protocol: str
    asset: str
    chain: str
    pool: str | None = None
    principal: Decimal
    accrued: Decimal | None = None


class Portfolio(StrictModel):
    # venue -> asset -> decimal-string balance
    balances: dict[str, dict[str, Decimal]] = Field(default_factory=dict)
    perp_positions: list[PerpPosition] = Field(default_factory=list)
    yield_positions: list[YieldPosition] = Field(default_factory=list)


class Snapshot(StrictModel):
    ts: datetime
    equity_usd: Decimal
    portfolio: Portfolio | None = None


class Event(OpenModel):
    ts: datetime
    type: EventType
    node_id: str | None = None
    reason: str | None = None
    detail: dict[str, Any] | None = None


class SimulationTrace(StrictModel):
    schema_version: str = "catalyst.backtest.trace.v1"
    policy: SimulationPolicy
    interval: Interval
    start: datetime
    end: datetime
    effective_start: datetime | None = None
    effective_end: datetime | None = None
    snapshots: list[Snapshot] = Field(default_factory=list)
    events: list[Event] = Field(default_factory=list)
    final_portfolio: Portfolio
    warnings: list[str] = Field(default_factory=list)
    errors: list[str] = Field(default_factory=list)


__all__ = [
    "SimulationTrace",
    "Snapshot",
    "Event",
    "EventType",
    "Portfolio",
    "PerpPosition",
    "YieldPosition",
]
