"""User-facing HTTP API for the backtest workbench.

Run lifecycle:

- ``POST /backtests`` — validate + run; returns id + status.
- ``GET /backtests/{id}`` — run status.
- ``GET /backtests/{id}/result`` — summarized result.
- ``GET /backtests/{id}/events`` — paginated, filterable event log.
- ``GET /backtests/{id}/metadata`` — run-level metadata.
- ``GET /backtests?graph_hash=...`` — compact run history for a graph.

Workbench setup (no run created):

- ``POST /backtests/preview`` — validate graph, summarize, data requirements, resolved policy.
- ``POST /market-data/coverage`` — series coverage + missing-data warnings before a run.
- ``GET /policy-profiles`` — Strict/Conservative/Research with resolved policies.

The app is built via :func:`create_app` with an injected simulation client,
market data source, and artifact store, so it is fully testable offline.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime

from fastapi import FastAPI, HTTPException, Query
from fastapi.responses import JSONResponse
from pydantic import BaseModel

from catalyst_backtest_worker import (
    ArtifactStore,
    InMemoryArtifactStore,
    RunRecord,
    SimulationClient,
    run_backtest,
)
from catalyst_contracts import BacktestRequest, Graph
from catalyst_contracts.request import Interval
from catalyst_graph_compiler import CompileError, compile_graph
from catalyst_market_data import build_bundle
from catalyst_market_data.sources import MarketDataSource

from .policies import list_profiles, resolve_request_policy
from .support import coverage_response, graph_hash, graph_summary


def _error(status: int, code: str, message: str, **extra) -> JSONResponse:
    return JSONResponse(
        status_code=status, content={"error": {"code": code, "message": message}, **extra}
    )


@dataclass
class _RunEntry:
    record: RunRecord
    graph_hash: str
    policy_profile: str
    config: dict
    created_at: str
    summary: dict = field(default_factory=dict)
    warning_count: int = 0


class PreviewRequest(BaseModel):
    graph: Graph
    policy: dict | None = None


class CoverageRequest(BaseModel):
    graph: Graph
    start: datetime
    end: datetime
    interval: Interval
    policy: dict | None = None


def create_app(
    *,
    client: SimulationClient,
    source: MarketDataSource,
    store: ArtifactStore | None = None,
    missing: str = "warn",
) -> FastAPI:
    """Build the API app with its dependencies injected."""

    store = store or InMemoryArtifactStore()
    runs: dict[str, _RunEntry] = {}
    app = FastAPI(title="Catalyst Backtest API", version="0.1.0")

    @app.get("/health")
    def health() -> dict:
        return {"status": "ok", "service": "catalyst-backtest-api"}

    # --- run lifecycle ---

    @app.post("/backtests", status_code=201)
    def create_backtest(request: BacktestRequest):
        record = run_backtest(request, client=client, source=source, store=store, missing=missing)
        summary = (record.result or {}).get("summary", {}) if record.ok else {}
        runs[record.id] = _RunEntry(
            record=record,
            graph_hash=graph_hash(request.graph),
            policy_profile=request.policy.profile,
            config={
                "start": request.config.start.isoformat(),
                "end": request.config.end.isoformat(),
                "interval": request.config.interval,
            },
            created_at=datetime.now(UTC).isoformat(),
            summary=summary,
            warning_count=len(record.warnings),
        )
        if not record.ok:
            return _error(422, "backtest_failed", record.error or "unknown error", id=record.id)
        return {"id": record.id, "status": record.status.value}

    @app.get("/backtests")
    def list_backtests(graph_hash: str | None = Query(default=None)) -> dict:
        items = []
        for entry in runs.values():
            if graph_hash is not None and entry.graph_hash != graph_hash:
                continue
            items.append(
                {
                    "id": entry.record.id,
                    "graph_hash": entry.graph_hash,
                    "status": entry.record.status.value,
                    "policy_profile": entry.policy_profile,
                    **entry.config,
                    "created_at": entry.created_at,
                    "summary": {
                        k: entry.summary.get(k)
                        for k in ("final_value_usd", "return_pct", "max_drawdown_pct")
                    },
                    "warning_count": entry.warning_count,
                }
            )
        items.sort(key=lambda r: r["created_at"])
        return {"items": items}

    @app.get("/backtests/{run_id}")
    def get_backtest(run_id: str) -> dict:
        entry = _require(runs, run_id)
        return {
            "id": entry.record.id,
            "status": entry.record.status.value,
            "error": entry.record.error,
        }

    @app.get("/backtests/{run_id}/result")
    def get_result(run_id: str):
        _require(runs, run_id)
        result = store.read_result(run_id)
        if result is None:
            return _error(
                409, "no_result", f"backtest {run_id!r} has no result (it may have failed)"
            )
        return result

    @app.get("/backtests/{run_id}/metadata")
    def get_metadata(run_id: str) -> dict:
        entry = _require(runs, run_id)
        result = store.read_result(run_id) or {}
        meta = result.get("metadata", {})
        return {
            "id": entry.record.id,
            "graph_hash": entry.graph_hash,
            "status": entry.record.status.value,
            "created_at": entry.created_at,
            "config": entry.config,
            "resolved_policy": meta.get("policy"),
            "data_coverage": meta.get("data_coverage", []),
            "warnings": meta.get("warnings", []) + list(entry.record.warnings),
            "artifacts": {
                "trace": entry.record.trace_ref,
                "result": entry.record.result_ref,
                "metadata": entry.record.metadata_ref,
            },
            "summary": entry.summary,
        }

    @app.get("/backtests/{run_id}/events")
    def get_events(
        run_id: str,
        type: str | None = Query(default=None),
        node_id: str | None = Query(default=None),
        status: str | None = Query(default=None),
        cursor: int = Query(default=0, ge=0),
        limit: int = Query(default=100, ge=1, le=1000),
    ) -> dict:
        _require(runs, run_id)
        trace = store.read_trace(run_id) or {}
        events = trace.get("events", [])

        # status is a convenience filter that maps onto action event types.
        status_type = {"executed": "action_executed", "rejected": "action_rejected"}.get(status)
        wanted_type = type or status_type

        filtered = [
            e
            for e in events
            if (wanted_type is None or e.get("type") == wanted_type)
            and (node_id is None or e.get("node_id") == node_id)
        ]
        page = filtered[cursor : cursor + limit]
        next_cursor = cursor + limit if cursor + limit < len(filtered) else None
        return {"items": page, "next_cursor": next_cursor, "total": len(filtered)}

    # --- workbench setup (no run created) ---

    @app.post("/backtests/preview")
    def preview(request: PreviewRequest) -> dict:
        resolved = resolve_request_policy(request.policy)
        ghash = graph_hash(request.graph)
        try:
            compiled = compile_graph(request.graph)
        except CompileError as exc:
            return {
                "graph_hash": ghash,
                "valid": False,
                "error": str(exc),
                "resolved_policy": resolved.model_dump(by_alias=True),
                "warnings": [],
            }
        return {
            "graph_hash": ghash,
            "valid": True,
            "graph_summary": graph_summary(request.graph, compiled),
            "data_requirements": compiled.data_requirements.model_dump(exclude_none=True),
            "resolved_policy": resolved.model_dump(by_alias=True),
            "warnings": list(compiled.warnings),
        }

    @app.post("/market-data/coverage")
    def coverage(request: CoverageRequest):
        try:
            compiled = compile_graph(request.graph)
        except CompileError as exc:
            return _error(422, "invalid_graph", str(exc))
        bundle = build_bundle(
            compiled,
            start=request.start,
            end=request.end,
            interval=request.interval,
            source=source,
            missing="warn",
        )
        return coverage_response(bundle)

    @app.get("/policy-profiles")
    def policy_profiles() -> dict:
        return {"items": list_profiles()}

    return app


def _require(runs: dict[str, _RunEntry], run_id: str) -> _RunEntry:
    entry = runs.get(run_id)
    if entry is None:
        raise HTTPException(status_code=404, detail=f"no backtest {run_id!r}")
    return entry


__all__ = ["create_app"]
