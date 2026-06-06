"""Shared Python contract models for Catalyst backtesting.

These Pydantic models are kept aligned with the language-neutral JSON Schemas in
the repo ``schemas/`` directory and mirror the Rust structs in
``crates/contracts``. Decimal/quantity values are carried as strings to preserve
precision across the Python <-> JSON <-> Rust boundary.
"""

from __future__ import annotations

from ._base import Decimal, OpenModel, StrictModel
from .graph import (
    Edge,
    Graph,
    Node,
    PerpOrderConfig,
    PriceThresholdConfig,
    SwapConfig,
    YieldConfig,
)
from .market_data import (
    Candle,
    CandleSeries,
    FundingSeries,
    GasSeries,
    MarketDataBundle,
    Provider,
    YieldSeries,
)
from .policy import PolicyProfile, SimulationPolicy
from .request import BacktestConfig, BacktestRequest, ExecutionOverrides, Interval, PolicySelector
from .result import (
    BacktestResult,
    Costs,
    DrawdownPoint,
    EquityPoint,
    ResultMetadata,
    Summary,
    Trade,
)
from .schemas import SCHEMA_FILES, load_schema, schemas_dir, validate
from .trace import Event, PerpPosition, Portfolio, SimulationTrace, Snapshot, YieldPosition

__version__ = "0.1.0"

__all__ = [
    "__version__",
    # base
    "Decimal",
    "StrictModel",
    "OpenModel",
    # graph
    "Graph",
    "Node",
    "Edge",
    "SwapConfig",
    "PerpOrderConfig",
    "YieldConfig",
    "PriceThresholdConfig",
    # policy
    "SimulationPolicy",
    "PolicyProfile",
    # request
    "BacktestRequest",
    "BacktestConfig",
    "PolicySelector",
    "ExecutionOverrides",
    "Interval",
    # market data
    "MarketDataBundle",
    "CandleSeries",
    "Candle",
    "FundingSeries",
    "GasSeries",
    "YieldSeries",
    "Provider",
    # trace
    "SimulationTrace",
    "Snapshot",
    "Event",
    "Portfolio",
    "PerpPosition",
    "YieldPosition",
    # result
    "BacktestResult",
    "Summary",
    "EquityPoint",
    "DrawdownPoint",
    "Trade",
    "Costs",
    "ResultMetadata",
    # schemas
    "SCHEMA_FILES",
    "load_schema",
    "schemas_dir",
    "validate",
]
