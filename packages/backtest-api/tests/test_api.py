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
    assert any(e["type"] == "action_executed" for e in events.json()["items"])


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
    assert client.get("/backtests/nope/metadata").status_code == 404


# --- workbench setup endpoints (#33) ---


def test_policy_profiles_lists_three_resolved() -> None:
    resp = make_client().get("/policy-profiles")
    assert resp.status_code == 200
    items = resp.json()["items"]
    ids = {p["id"] for p in items}
    assert ids == {"strict_v1", "conservative_v1", "research_v1"}
    strict = next(p for p in items if p["id"] == "strict_v1")
    assert strict["resolved_policy"]["fills"]["price_selection"] == "close"
    conservative = next(p for p in items if p["id"] == "conservative_v1")
    assert conservative["resolved_policy"]["fills"]["price_selection"] == "worse_side_ohlc"
    assert conservative["resolved_policy"]["fills"]["slippage"]["bps"] == "25"


def test_preview_valid_graph() -> None:
    resp = make_client().post(
        "/backtests/preview",
        json={
            "graph": copy.deepcopy(SAMPLE_GRAPHS["g11_evm_swap_if_below"]),
            "policy": {"profile": "conservative_v1"},
        },
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["valid"] is True
    assert body["graph_hash"]
    assert body["graph_summary"]["signals"] == ["eth-below-1800"]
    assert body["graph_summary"]["actions"] == ["buy-eth-on-base"]
    assert {(c["venue"], c["symbol"]) for c in body["data_requirements"]["candles"]} == {
        ("base", "ETH")
    }
    assert body["resolved_policy"]["profile"] == "conservative_v1"


def test_preview_invalid_graph_is_not_an_error() -> None:
    bad = copy.deepcopy(SAMPLE_GRAPHS["g01_evm_swap_buy_eth_base"])
    bad["edges"] = [{"from": "buy-eth-on-base", "to": "ghost"}]
    resp = make_client().post("/backtests/preview", json={"graph": bad})
    assert resp.status_code == 200  # preview reports invalidity, doesn't error
    body = resp.json()
    assert body["valid"] is False
    assert "ghost" in body["error"]
    # still returns a resolved policy + a stable hash for the UI
    assert body["resolved_policy"]["profile"] == "strict_v1"
    assert body["graph_hash"]


def test_coverage_reports_series_and_warnings() -> None:
    resp = make_client().post(
        "/market-data/coverage",
        json={
            "graph": copy.deepcopy(SAMPLE_GRAPHS["g01_evm_swap_buy_eth_base"]),
            "start": "2024-01-01T00:00:00Z",
            "end": "2024-01-01T02:00:00Z",
            "interval": "1h",
        },
    )
    assert resp.status_code == 200
    body = resp.json()
    kinds = {(r["kind"], r.get("venue") or r.get("chain")) for r in body["coverage"]}
    assert ("candles", "base") in kinds
    assert ("gas", "base") in kinds
    # the fixture bundle covers base ETH candles + gas, so it's complete
    assert all(r["complete"] for r in body["coverage"])
    assert body["warnings"] == []


def test_coverage_warns_on_missing_data() -> None:
    from catalyst_market_data import FixtureSource

    empty = FixtureSource.from_dict(
        {"interval": "1h", "start": "2024-01-01T00:00:00Z", "end": "2024-01-01T02:00:00Z"}
    )
    app = create_app(client=CallableSimulationClient(fake_simulate), source=empty)
    client = TestClient(app)
    resp = client.post(
        "/market-data/coverage",
        json={
            "graph": copy.deepcopy(SAMPLE_GRAPHS["g01_evm_swap_buy_eth_base"]),
            "start": "2024-01-01T00:00:00Z",
            "end": "2024-01-01T02:00:00Z",
            "interval": "1h",
        },
    )
    body = resp.json()
    assert any("candles for ETH on base" in w for w in body["warnings"])
    assert not all(r["complete"] for r in body["coverage"])


def test_run_history_by_graph_hash() -> None:
    client = make_client()
    # two runs of the same graph + one of a different graph
    r1 = client.post("/backtests", json=request_body("g01_evm_swap_buy_eth_base")).json()["id"]
    r2 = client.post("/backtests", json=request_body("g01_evm_swap_buy_eth_base")).json()["id"]
    other = request_body("g02_hl_spot_buy_eth")
    other["config"]["initial_portfolio"] = {"hyperliquid": {"USDC": "1000"}}
    client.post("/backtests", json=other)

    ghash = client.post(
        "/backtests/preview", json={"graph": SAMPLE_GRAPHS["g01_evm_swap_buy_eth_base"]}
    ).json()["graph_hash"]

    items = client.get(f"/backtests?graph_hash={ghash}").json()["items"]
    assert {i["id"] for i in items} == {r1, r2}
    assert all(i["graph_hash"] == ghash for i in items)
    assert items[0]["summary"]["final_value_usd"] is not None

    # unfiltered lists all three
    assert len(client.get("/backtests").json()["items"]) == 3


def multi_event_simulate(payload: dict) -> dict:
    cfg = payload["config"]
    return {
        "schema_version": "catalyst.backtest.trace.v1",
        "policy": {"schema_version": "catalyst.backtest.policy.v1", **payload["policy"]},
        "interval": cfg["interval"],
        "start": cfg["start"],
        "end": cfg["end"],
        "snapshots": [{"ts": cfg["start"], "equity_usd": "1000"}],
        "events": [
            {"ts": cfg["start"], "type": "signal_fired", "node_id": "sig"},
            {
                "ts": cfg["start"],
                "type": "action_executed",
                "node_id": "buy",
                "detail": {"kind": "swap"},
            },
            {"ts": cfg["start"], "type": "action_rejected", "node_id": "sell", "reason": "x"},
            {
                "ts": cfg["start"],
                "type": "action_executed",
                "node_id": "buy2",
                "detail": {"kind": "swap"},
            },
        ],
        "final_portfolio": {"balances": {}, "perp_positions": [], "yield_positions": []},
        "warnings": [],
        "errors": [],
    }


def test_events_filter_and_paginate() -> None:
    app = create_app(
        client=CallableSimulationClient(multi_event_simulate),
        source=FixtureSource.from_file(FIXTURE_BUNDLE),
    )
    client = TestClient(app)
    run_id = client.post("/backtests", json=request_body()).json()["id"]

    allev = client.get(f"/backtests/{run_id}/events").json()
    assert allev["total"] == 4
    assert allev["next_cursor"] is None

    assert client.get(f"/backtests/{run_id}/events?type=action_executed").json()["total"] == 2
    assert client.get(f"/backtests/{run_id}/events?status=rejected").json()["total"] == 1
    assert client.get(f"/backtests/{run_id}/events?node_id=buy").json()["total"] == 1

    page1 = client.get(f"/backtests/{run_id}/events?limit=2").json()
    assert len(page1["items"]) == 2
    assert page1["next_cursor"] == 2
    page2 = client.get(f"/backtests/{run_id}/events?limit=2&cursor=2").json()
    assert len(page2["items"]) == 2
    assert page2["next_cursor"] is None


def test_metadata_endpoint() -> None:
    client = make_client()
    run_id = client.post("/backtests", json=request_body()).json()["id"]
    meta = client.get(f"/backtests/{run_id}/metadata").json()
    assert meta["id"] == run_id
    assert meta["graph_hash"]
    assert meta["status"] == "succeeded"
    assert meta["resolved_policy"]["profile"] == "strict_v1"
    assert meta["config"]["interval"] == "1h"
    assert "artifacts" in meta
