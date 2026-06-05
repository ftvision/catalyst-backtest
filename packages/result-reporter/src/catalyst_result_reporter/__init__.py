"""Result reporting package for Catalyst backtesting.

Turns a raw ``SimulationTrace`` into a user-facing ``BacktestResult``: summary,
equity/drawdown curves, trade log, costs breakdown, and the resolved assumptions
plus data-coverage metadata.
"""

from __future__ import annotations

from .reporter import summarize

__version__ = "0.1.0"

__all__ = ["__version__", "summarize"]
