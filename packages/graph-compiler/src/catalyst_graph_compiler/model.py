"""Compiled graph representation produced by the graph compiler.

This is a normalized, deterministic, serializable form of a Catalyst graph that
the market-data planner and the simulation engine consume. It is intentionally
*not* part of `schemas/`/`catalyst-contracts` yet: it is an internal artifact of
the compiler whose shape may evolve alongside the engine.
"""

from __future__ import annotations

from typing import Any, Literal

from catalyst_contracts import StrictModel
from pydantic import Field

TriggerType = Literal["initial", "signal", "action"]


class Trigger(StrictModel):
    """Why/when an action runs.

    - ``initial``: action has no incoming edges; runs once at start.
    - ``signal``: action runs when the source signal fires.
    - ``action``: action runs immediately after the source action succeeds.
    """

    type: TriggerType
    source_id: str | None = None


class CompiledAction(StrictModel):
    id: str
    subtype: str
    config: dict[str, Any]
    triggers: list[Trigger]


class CompiledSignal(StrictModel):
    id: str
    subtype: str
    config: dict[str, Any]
    targets: list[str] = Field(default_factory=list)


class CandleRequirement(StrictModel):
    venue: str
    symbol: str


class FundingRequirement(StrictModel):
    venue: str
    symbol: str


class GasRequirement(StrictModel):
    chain: str


class YieldRequirement(StrictModel):
    protocol: str
    asset: str
    chain: str
    pool: str | None = None


class DataRequirements(StrictModel):
    """Everything the market-data package must source to run this graph."""

    candles: list[CandleRequirement] = Field(default_factory=list)
    funding: list[FundingRequirement] = Field(default_factory=list)
    gas: list[GasRequirement] = Field(default_factory=list)
    yields: list[YieldRequirement] = Field(default_factory=list)


class CompiledGraph(StrictModel):
    schema_version: str = "catalyst.backtest.compiled_graph.v1"
    actions: list[CompiledAction] = Field(default_factory=list)
    signals: list[CompiledSignal] = Field(default_factory=list)
    data_requirements: DataRequirements = Field(default_factory=DataRequirements)
    warnings: list[str] = Field(default_factory=list)


__all__ = [
    "Trigger",
    "TriggerType",
    "CompiledAction",
    "CompiledSignal",
    "CandleRequirement",
    "FundingRequirement",
    "GasRequirement",
    "YieldRequirement",
    "DataRequirements",
    "CompiledGraph",
]
