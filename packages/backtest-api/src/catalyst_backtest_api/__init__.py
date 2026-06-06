"""Backtest API package for Catalyst backtesting.

Exposes the user-facing HTTP endpoints for creating and inspecting backtest runs,
handing work off to the worker layer. Build the app with :func:`create_app`.
"""

from __future__ import annotations

from .app import create_app
from .policies import list_profiles, resolve_profile

__version__ = "0.1.0"

__all__ = ["__version__", "create_app", "list_profiles", "resolve_profile"]
