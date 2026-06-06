"""Tests for the backtest API (offline, via FastAPI TestClient)."""

from __future__ import annotations

import copy
import json
from decimal import Decimal
from pathlib import Path

from fastapi.testclient import TestClient

from catalyst_backtest_api import create_app
from catalyst_backtest_worker import CallableSimulationClient
from catalyst_market_data import FixtureSource


def _repo_root() -> Path:
    for parent in Path(__file__).resolve().parents:
        if (parent / "tests" / "fixtures" / "sample_graphs.json").exists():
            return parent
    raise RuntimeError("repo root not found")


ROOT = _repo_root()
SAMPLE_GRAPHS = json.loads((ROOT / "tests" / "fixtures" / "sample_graphs.json").read_text())
FIXTURE_BUNDLE = ROOT / "tests" / "fixtures" / "market_data" / "eth_2h.json"


def fake_simulate(payload: dict) -> dict:
    cfg = payload["config"]
    equity = sum(
        Decimal(v) for assets in cfg["initial_portfolio"].values() for v in assets.values()
    )
    return {
        "schema_version": "catalyst.backtest.trace.v1",
        "policy": {"schema_version": "catalyst.backtest.policy.v1", **payload["policy"]},
        "interval": cfg["interval"],
        "start": cfg["start"],
        "end": cfg["end"],
        "snapshots": [{"ts": cfg["start"], "equity_usd": str(equity)}],
        "events": [
            {
                "ts": cfg["start"],
                "type": "action_executed",
                "node_id": "buy-eth-on-base",
                "detail": {"kind": "swap"},
            }
        ],
        "final_portfolio": {
            "balances": {"base": {"USDC": "900"}},
            "perp_positions": [],
            "yield_positions": [],
        },
        "warnings": [],
        "errors": [],
    }


def make_client() -> TestClient:
    app = create_app(
        client=CallableSimulationClient(fake_simulate),
        source=FixtureSource.from_file(FIXTURE_BUNDLE),
    )
    return TestClient(app)


def request_body(graph_name: str = "g01_evm_swap_buy_eth_base") -> dict:
    return {
        "graph": copy.deepcopy(SAMPLE_GRAPHS[graph_name]),
        "policy": {"profile": "strict_v1"},
        "config": {
            "start": "2024-01-01T00:00:00Z",
            "end": "2024-01-01T02:00:00Z",
            "interval": "1h",
            "initial_portfolio": {"base": {"USDC": "1000"}},
        },
    }


def test_health() -> None:
    resp = make_client().get("/health")
    assert resp.status_code == 200
    assert resp.json()["status"] == "ok"


def test_create_then_inspect_full_lifecycle() -> None:
    client = make_client()

    created = client.post("/backtests", json=request_body())
    assert created.status_code == 201
    run_id = created.json()["id"]
    assert created.json()["status"] == "succeeded"

    status = client.get(f"/backtests/{run_id}")
    assert status.status_code == 200
    assert status.json()["status"] == "succeeded"

    result = client.get(f"/backtests/{run_id}/result")
    assert result.status_code == 200
    body = result.json()
    assert "summary" in body
    assert body["metadata"]["policy"]["profile"] == "strict_v1"

    events = client.get(f"/backtests/{run_id}/events")
    assert events.status_code == 200
    assert any(e["type"] == "action_executed" for e in events.json()["events"])


def test_uses_contract_models_for_validation() -> None:
    client = make_client()
    bad = request_body()
    del bad["config"]["interval"]  # required by BacktestConfig
    resp = client.post("/backtests", json=bad)
    assert resp.status_code == 422  # FastAPI/pydantic validation


def test_invalid_graph_returns_stable_error() -> None:
    client = make_client()
    bad = request_body()
    bad["graph"]["edges"] = [{"from": "buy-eth-on-base", "to": "missing-node"}]
    resp = client.post("/backtests", json=bad)
    assert resp.status_code == 422
    body = resp.json()
    assert body["error"]["code"] == "backtest_failed"
    assert "missing-node" in body["error"]["message"]


def test_unknown_run_is_404() -> None:
    client = make_client()
    assert client.get("/backtests/nope").status_code == 404
    assert client.get("/backtests/nope/result").status_code == 404
    assert client.get("/backtests/nope/events").status_code == 404
