"""Run-file loading: resolution, validation, and CLI overrides."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from catalyst_client.config import load_run

GRAPH = {
    "schema_version": "catalyst.graph.definition.v1",
    "nodes": [
        {
            "id": "open",
            "kind": "action",
            "subtype": "perp_order",
            "config": {
                "symbol": "ETH",
                "side": "long",
                "size_usd": "500",
                "leverage": "2",
                "chain": "hyperliquid",
                "order_type": "market",
                "reduce_only": False,
            },
        }
    ],
    "edges": [],
}


def _write_run(tmp_path: Path, *, graph_ref: str = "graph.json") -> Path:
    (tmp_path / "graph.json").write_text(json.dumps(GRAPH))
    run = tmp_path / "run.toml"
    run.write_text(
        f"""
graph = "{graph_ref}"
policy = "strict_v1"

[config]
start = "2026-01-01T00:00:00Z"
end = "2026-02-01T00:00:00Z"
interval = "1h"

[config.initial_portfolio.hyperliquid]
USDC = "10000"
"""
    )
    return run


def test_load_run_resolves_graph_and_builds_body(tmp_path: Path) -> None:
    spec = load_run(_write_run(tmp_path))
    body = spec.body()
    assert set(body) >= {"graph", "config", "policy"}
    assert body["policy"] == {"profile": "strict_v1"}
    assert body["graph"]["nodes"][0]["id"] == "open"
    assert body["config"]["interval"] == "1h"
    assert body["config"]["initial_portfolio"]["hyperliquid"]["USDC"] == "10000"
    # Convenience views normalize to RFC3339 'Z'.
    assert spec.start == "2026-01-01T00:00:00Z"
    assert spec.policy_profile == "strict_v1"


def test_overrides_win_over_file(tmp_path: Path) -> None:
    spec = load_run(
        _write_run(tmp_path),
        overrides={
            "interval": "4h",
            "policy": "research_v1",
            "start": None,
            "end": None,
            "graph": None,
        },
    )
    assert spec.interval == "4h"
    assert spec.policy_profile == "research_v1"


def test_inline_graph_table(tmp_path: Path) -> None:
    # Inline graph via TOML is awkward, so the common case is a path; here we
    # confirm a dict graph (e.g. injected post-parse) also validates.
    spec = load_run(
        _write_run(tmp_path),
        overrides={"graph": GRAPH, "start": None, "end": None, "interval": None, "policy": None},
    )
    assert spec.graph["nodes"][0]["subtype"] == "perp_order"


def test_missing_graph_file_is_an_error(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError):
        load_run(_write_run(tmp_path, graph_ref="nope.json"))
