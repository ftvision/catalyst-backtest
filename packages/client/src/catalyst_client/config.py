"""Load a ``run.toml`` into a validated backtest request.

A *run file* is the unit of a backtest setup: it references a graph (by path or
inline) and carries the rest of the configuration the service needs that the
graph alone doesn't express — the period, interval, starting portfolio, policy
profile, and execution overrides.

The file is parsed with the stdlib ``tomllib`` and validated through the
``catalyst-contracts`` Pydantic models, so the client rejects a malformed run
*before* hitting the network and never re-defines the contract.

Example ``run.toml``::

    graph = "strategies/graphs/g05_hl_perp_open_long.json"  # path (rel. to file) or inline [graph]
    policy = "strict_v1"

    [config]
    start = "2024-01-01T00:00:00Z"
    end   = "2024-06-01T00:00:00Z"
    interval = "1h"

    [config.initial_portfolio.hyperliquid]
    USDC = "10000"

    [config.execution]
    slippage_bps = "10"
    gas_model = "historical"
"""

from __future__ import annotations

import json
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from catalyst_contracts import BacktestRequest


@dataclass
class RunSpec:
    """A validated run: the request body to POST plus a few convenience views."""

    request: BacktestRequest
    market_data: dict[str, Any] | None = None

    def body(self) -> dict[str, Any]:
        """The JSON request body for ``POST /backtests`` / ``/simulate``."""
        body = self.request.model_dump(mode="json", exclude_none=True)
        if self.market_data is not None:
            body["market_data"] = self.market_data
        return body

    @property
    def interval(self) -> str:
        return self.request.config.interval

    @property
    def start(self) -> str:
        return self.request.config.start.isoformat().replace("+00:00", "Z")

    @property
    def end(self) -> str:
        return self.request.config.end.isoformat().replace("+00:00", "Z")

    @property
    def policy_profile(self) -> str:
        return self.request.policy.profile

    @property
    def graph(self) -> dict[str, Any]:
        return self.request.graph.model_dump(mode="json", exclude_none=True)


def _resolve_graph(raw: Any, base_dir: Path) -> dict[str, Any]:
    """A graph reference is either an inline table or a path to a graph JSON."""
    if isinstance(raw, dict):
        return raw
    if isinstance(raw, str):
        path = (base_dir / raw).resolve()
        if not path.is_file():
            raise FileNotFoundError(f"graph file not found: {path}")
        return json.loads(path.read_text())
    raise ValueError("`graph` must be a path string or an inline table")


def _apply_overrides(data: dict[str, Any], overrides: dict[str, Any]) -> None:
    """Apply non-None CLI overrides onto the parsed run dict (in place)."""
    config = data.setdefault("config", {})
    for key in ("start", "end", "interval"):
        if overrides.get(key) is not None:
            config[key] = overrides[key]
    if overrides.get("policy") is not None:
        data["policy"] = {"profile": overrides["policy"]}
    if overrides.get("graph") is not None:
        # An explicit --graph path/dir wins over the file's reference.
        data["graph"] = overrides["graph"]


def load_run(
    path: str | Path,
    *,
    overrides: dict[str, Any] | None = None,
    market_data_path: str | Path | None = None,
) -> RunSpec:
    """Parse and validate a ``run.toml`` into a :class:`RunSpec`.

    ``overrides`` may carry ``start``/``end``/``interval``/``policy``/``graph``
    (typically from CLI flags); each wins over the file when not ``None``.
    """
    run_path = Path(path)
    data = tomllib.loads(run_path.read_text())

    overrides = overrides or {}
    _apply_overrides(data, overrides)

    if "graph" not in data:
        raise ValueError("run file must reference a `graph` (path or inline table)")
    if "config" not in data:
        raise ValueError("run file must contain a [config] table")

    base_dir = run_path.parent
    data["graph"] = _resolve_graph(data["graph"], base_dir)

    # Normalize a bare `policy = "profile"` string into the selector shape.
    policy = data.get("policy")
    if isinstance(policy, str):
        data["policy"] = {"profile": policy}

    request = BacktestRequest.model_validate(data)

    market_data = None
    if market_data_path is not None:
        market_data = json.loads(Path(market_data_path).read_text())

    return RunSpec(request=request, market_data=market_data)
