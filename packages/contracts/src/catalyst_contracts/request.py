"""Backtest request contract models (backtest-request.schema.json)."""

from __future__ import annotations

from datetime import datetime
from typing import Literal

from pydantic import Field

from ._base import Decimal, StrictModel
from .graph import Graph
from .policy import PolicyProfile

Interval = Literal["1m", "5m", "15m", "1h", "4h", "1d"]


class PolicySelector(StrictModel):
    """A bare profile selector; a fully resolved policy may also be supplied."""

    profile: PolicyProfile = "strict_v1"


class ExecutionOverrides(StrictModel):
    signal_trigger: (
        Literal["level", "crossing", "crossing_with_cooldown", "once_per_backtest"] | None
    ) = None
    slippage_bps: Decimal | None = None
    gas_model: (
        Literal["none", "fixed_usd", "fixed_native", "historical_fee_history", "historical"] | None
    ) = None
    action_cooldown: str | None = None


class BacktestConfig(StrictModel):
    start: datetime
    end: datetime
    interval: Interval
    # venue -> asset -> decimal-string amount
    initial_portfolio: dict[str, dict[str, Decimal]]
    execution: ExecutionOverrides | None = None


class BacktestRequest(StrictModel):
    schema_version: str = "catalyst.backtest.request.v1"
    graph: Graph
    policy: PolicySelector = Field(default_factory=PolicySelector)
    config: BacktestConfig


__all__ = [
    "BacktestRequest",
    "BacktestConfig",
    "PolicySelector",
    "ExecutionOverrides",
    "Interval",
]
