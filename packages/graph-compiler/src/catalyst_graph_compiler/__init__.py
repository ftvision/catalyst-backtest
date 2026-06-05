"""Graph compiler package for Catalyst backtesting.

Validates and normalizes a raw Catalyst graph into a :class:`CompiledGraph`:
typed operations, execution triggers (initial / signal-driven / action-chained),
and the data requirements the market-data package must source.
"""

from __future__ import annotations

from .compiler import DEFAULT_PRICE_VENUE, STABLE_ASSETS, compile_graph
from .errors import CompileError
from .model import (
    CandleRequirement,
    CompiledAction,
    CompiledGraph,
    CompiledSignal,
    DataRequirements,
    FundingRequirement,
    GasRequirement,
    Trigger,
    YieldRequirement,
)

__version__ = "0.1.0"

__all__ = [
    "__version__",
    "compile_graph",
    "CompileError",
    "STABLE_ASSETS",
    "DEFAULT_PRICE_VENUE",
    "CompiledGraph",
    "CompiledAction",
    "CompiledSignal",
    "Trigger",
    "DataRequirements",
    "CandleRequirement",
    "FundingRequirement",
    "GasRequirement",
    "YieldRequirement",
]
