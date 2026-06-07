"""Python CLI client for the Catalyst backtest service.

The deterministic run path is the Rust service (see ADR 0001); this package is a
thin, typed client over its HTTP API. It reuses the schema-aligned Pydantic
models from ``catalyst-contracts`` for request building and result parsing, so
nothing here re-defines the contract.

Entry points:
- :class:`catalyst_client.api.CatalystClient` — the HTTP client.
- :func:`catalyst_client.config.load_run` — load a ``run.toml`` into a request.
- ``catalyst-bt`` — the Typer CLI (``catalyst_client.cli``).
"""

from __future__ import annotations

from .api import ApiError, CatalystClient
from .config import RunSpec, load_run

__all__ = ["CatalystClient", "ApiError", "RunSpec", "load_run"]
