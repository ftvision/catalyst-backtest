"""Simulation policy contract models (simulation-policy.schema.json)."""

from __future__ import annotations

from typing import Literal

from pydantic import Field

from ._base import Decimal, StrictModel

PolicyProfile = Literal["strict_v1", "conservative_v1", "research_v1"]


class BalancePolicy(StrictModel):
    insufficient_balance: Literal[
        "reject", "partial_fill", "clamp_to_available", "allow_negative"
    ] = "reject"


class SlippagePolicy(StrictModel):
    model: Literal["fixed_bps", "volume_based", "amm_price_impact", "none"] = "fixed_bps"
    bps: Decimal = "10"


class FeePolicy(StrictModel):
    model: Literal["fixed_bps", "venue_fee_table", "none"] = "fixed_bps"
    bps: Decimal = "5"


class FillsPolicy(StrictModel):
    partial_fills: Literal["none", "allow_if_configured", "always_allow"] = "none"
    price_selection: Literal["close", "open", "mid", "next_open", "worse_side_ohlc"] = "close"
    slippage: SlippagePolicy = Field(default_factory=SlippagePolicy)
    fees: FeePolicy = Field(default_factory=FeePolicy)


class GasFallback(StrictModel):
    model: Literal["none", "fixed_usd", "fixed_native"] = "fixed_usd"
    amount: Decimal = "0.25"


class GasPolicy(StrictModel):
    model: Literal["none", "fixed_usd", "fixed_native", "historical_fee_history"] = (
        "historical_fee_history"
    )
    fallback: GasFallback = Field(default_factory=GasFallback)


class SignalPolicy(StrictModel):
    trigger: Literal["level", "crossing", "crossing_with_cooldown", "once_per_backtest"] = (
        "crossing"
    )
    repeat: Literal["never", "on_each_signal_fire", "with_cooldown", "max_count"] = (
        "on_each_signal_fire"
    )
    cooldown: str | None = None


class OrderingPolicy(StrictModel):
    same_tick: Literal[
        "graph_order",
        "topological_order",
        "signals_first_then_actions",
        "conservative_adverse_order",
    ] = "topological_order"


class DataPolicy(StrictModel):
    missing_required: Literal["fail", "skip_tick", "forward_fill"] = "fail"
    missing_optional: Literal["warn", "fail", "forward_fill", "fallback_provider"] = "warn"


class PerpPolicy(StrictModel):
    liquidation_check: Literal["every_tick", "never"] = "every_tick"
    funding: Literal["historical", "none"] = "historical"
    reduce_only_validation: Literal["strict", "lenient"] = "strict"


class YieldPolicy(StrictModel):
    accrual: Literal["simple_apr", "compound_apy", "protocol_index"] = "simple_apr"


class SimulationPolicy(StrictModel):
    schema_version: str = "catalyst.backtest.policy.v1"
    profile: PolicyProfile
    balance: BalancePolicy = Field(default_factory=BalancePolicy)
    fills: FillsPolicy = Field(default_factory=FillsPolicy)
    gas: GasPolicy = Field(default_factory=GasPolicy)
    signals: SignalPolicy = Field(default_factory=SignalPolicy)
    ordering: OrderingPolicy = Field(default_factory=OrderingPolicy)
    data: DataPolicy = Field(default_factory=DataPolicy)
    perps: PerpPolicy = Field(default_factory=PerpPolicy)
    yield_: YieldPolicy = Field(default_factory=YieldPolicy, alias="yield")

    model_config = {"populate_by_name": True, "extra": "forbid"}


__all__ = ["SimulationPolicy", "PolicyProfile"]
