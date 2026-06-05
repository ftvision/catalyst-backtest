"""End-to-end (offline) tests for the backtest worker."""

from __future__ import annotations

import json
from decimal import Decimal
from pathlib import Path

import pytest

from catalyst_backtest_worker import (
    CallableSimulationClient,
    FileArtifactStore,
    HttpSimulationClient,
    InMemoryArtifactStore,
    RunStatus,
    run_backtest,
)
from catalyst_market_data import FixtureSource


def _repo_root() -> Path:
    for parent in Path(__file__).resolve().parents:
        if (parent / "tests" / "fixtures" / "sample_graphs.json").exists():
            return parent
    raise RuntimeError("repo root not found")


ROOT = _repo_root()
SAMPLE_GRAPHS = json.loads((ROOT / "tests" / "fixtures" / "sample_graphs.json").read_text())
FIXTURE_BUNDLE = ROOT / "tests" / "fixtures" / "market_data" / "eth_2h.json"


def fixture_source() -> FixtureSource:
    return FixtureSource.from_file(FIXTURE_BUNDLE)


def request_for(graph_name: str) -> dict:
    import copy

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


def fake_simulate(payload: dict) -> dict:
    """A stand-in for the Rust engine: returns a minimal valid trace."""
    cfg = payload["config"]
    start_equity = sum(
        Decimal(v) for assets in cfg["initial_portfolio"].values() for v in assets.values()
    )
    policy = {"schema_version": "catalyst.backtest.policy.v1", **payload["policy"]}
    return {
        "schema_version": "catalyst.backtest.trace.v1",
        "policy": policy,
        "interval": cfg["interval"],
        "start": cfg["start"],
        "end": cfg["end"],
        "snapshots": [{"ts": cfg["start"], "equity_usd": str(start_equity)}],
        "events": [
            {
                "ts": cfg["start"],
                "type": "action_executed",
                "node_id": "buy-eth-on-base",
                "detail": {"kind": "swap", "fee_usd": "0.05", "gas_usd": "0.02"},
            }
        ],
        "final_portfolio": {
            "balances": {"base": {"USDC": "900", "ETH": "0.05"}},
            "perp_positions": [],
            "yield_positions": [],
        },
        "warnings": [],
        "errors": [],
    }


def fake_client() -> CallableSimulationClient:
    return CallableSimulationClient(fake_simulate)


# --- End-to-end run ---


def test_run_persists_trace_and_result_separately() -> None:
    store = InMemoryArtifactStore()
    record = run_backtest(
        request_for("g01_evm_swap_buy_eth_base"),
        client=fake_client(),
        source=fixture_source(),
        store=store,
        run_id="run-1",
    )
    assert record.status is RunStatus.SUCCEEDED
    assert record.ok
    # raw trace and summarized result are stored separately
    trace = store.read_trace("run-1")
    result = store.read_result("run-1")
    assert trace is not None and result is not None
    assert "snapshots" in trace
    assert "summary" in result
    assert trace != result


def test_run_records_policy_and_provider_metadata() -> None:
    store = InMemoryArtifactStore()
    record = run_backtest(
        request_for("g01_evm_swap_buy_eth_base"),
        client=fake_client(),
        source=fixture_source(),
        store=store,
        run_id="run-2",
    )
    meta = store.metadata["run-2"]
    assert meta["policy"]["profile"] == "strict_v1"
    # providers come from the market data bundle (candles + gas for an EVM swap)
    kinds = {p["kind"] for p in meta["providers"]}
    assert {"candles", "gas"} <= kinds
    # result also carries the policy + coverage through the reporter
    assert record.result["metadata"]["policy"]["profile"] == "strict_v1"
    assert record.result["metadata"]["data_coverage"] == meta["providers"]


def test_file_store_writes_three_artifacts(tmp_path) -> None:
    store = FileArtifactStore(tmp_path)
    record = run_backtest(
        request_for("g01_evm_swap_buy_eth_base"),
        client=fake_client(),
        source=fixture_source(),
        store=store,
        run_id="run-3",
    )
    assert record.ok
    run_dir = tmp_path / "run-3"
    assert (run_dir / "trace.json").exists()
    assert (run_dir / "result.json").exists()
    assert (run_dir / "metadata.json").exists()


# --- Error propagation ---


def test_compile_error_becomes_failed_run() -> None:
    bad = request_for("g01_evm_swap_buy_eth_base")
    # break the swap config: remove the required amount
    del bad["graph"]["nodes"][0]["config"]["amount"]
    store = InMemoryArtifactStore()
    record = run_backtest(bad, client=fake_client(), source=fixture_source(), store=store)
    assert record.status is RunStatus.FAILED
    assert record.error
    assert store.results == {}  # nothing persisted on failure


def test_missing_required_data_becomes_failed_run() -> None:
    empty = FixtureSource.from_dict(
        {"interval": "1h", "start": "2024-01-01T00:00:00Z", "end": "2024-01-01T02:00:00Z"}
    )
    record = run_backtest(
        request_for("g01_evm_swap_buy_eth_base"),
        client=fake_client(),
        source=empty,
        missing="fail",
    )
    assert record.status is RunStatus.FAILED


def test_client_error_becomes_failed_run() -> None:
    def boom(payload: dict) -> dict:
        raise RuntimeError("service unavailable")

    record = run_backtest(
        request_for("g01_evm_swap_buy_eth_base"),
        client=CallableSimulationClient(boom),
        source=fixture_source(),
    )
    assert record.status is RunStatus.FAILED
    assert "service unavailable" in record.error


# --- HTTP client payload building ---


def test_http_client_builds_payload_and_parses_trace() -> None:
    calls: list[tuple[str, dict]] = []

    def transport(url: str, body: dict) -> dict:
        calls.append((url, body))
        return fake_simulate(body)

    client = HttpSimulationClient("http://sim:8080", transport=transport)
    record = run_backtest(
        request_for("g01_evm_swap_buy_eth_base"),
        client=client,
        source=fixture_source(),
        run_id="run-http",
    )
    assert record.ok
    url, body = calls[0]
    assert url == "http://sim:8080/simulate"
    assert set(body) == {"graph", "config", "policy", "market_data"}
    assert body["policy"] == {"profile": "strict_v1"}
    assert body["market_data"]["candles"]  # bundle was passed through


def test_http_client_without_transport_refuses_network() -> None:
    from catalyst_backtest_worker import NetworkDisabledError

    client = HttpSimulationClient("http://sim:8080")
    record = run_backtest(
        request_for("g01_evm_swap_buy_eth_base"),
        client=client,
        source=fixture_source(),
    )
    # the network error is captured as a failed run
    assert record.status is RunStatus.FAILED
    assert "transport" in record.error or "network" in record.error.lower()
    # and the client raises directly when used outside the worker
    from catalyst_contracts import MarketDataBundle

    bundle = MarketDataBundle.model_validate(json.loads(FIXTURE_BUNDLE.read_text()))
    with pytest.raises(NetworkDisabledError):
        client.simulate(graph={}, config={}, policy={}, market_data=bundle)
