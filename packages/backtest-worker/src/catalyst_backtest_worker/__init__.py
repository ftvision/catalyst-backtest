"""Backtest worker package for Catalyst backtesting.

Coordinates a full run across the graph compiler, market data, the Rust
simulation service, and the result reporter, persisting the raw trace and the
summarized result separately.
"""

from __future__ import annotations

from .artifacts import ArtifactStore, FileArtifactStore, InMemoryArtifactStore
from .client import (
    CallableSimulationClient,
    HttpSimulationClient,
    NetworkDisabledError,
    SimulationClient,
)
from .worker import RunRecord, RunStatus, run_backtest

__version__ = "0.1.0"

__all__ = [
    "__version__",
    "run_backtest",
    "RunRecord",
    "RunStatus",
    "SimulationClient",
    "CallableSimulationClient",
    "HttpSimulationClient",
    "NetworkDisabledError",
    "ArtifactStore",
    "InMemoryArtifactStore",
    "FileArtifactStore",
]
