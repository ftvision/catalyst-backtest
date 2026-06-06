"""User-facing HTTP API for creating and inspecting backtest runs.

Endpoints:

- ``POST /backtests`` — validate a request (a ``BacktestRequest`` contract model)
  and hand it to the worker; returns the run id + status.
- ``GET /backtests/{id}`` — run status.
- ``GET /backtests/{id}/result`` — the summarized result.
- ``GET /backtests/{id}/events`` — the raw event log from the trace.

The app is built via :func:`create_app` with an injected simulation client,
market data source, and artifact store, so it is fully testable offline.
"""

from __future__ import annotations

from fastapi import FastAPI, HTTPException
from fastapi.responses import JSONResponse

from catalyst_backtest_worker import (
    ArtifactStore,
    InMemoryArtifactStore,
    RunRecord,
    SimulationClient,
    run_backtest,
)
from catalyst_contracts import BacktestRequest
from catalyst_market_data.sources import MarketDataSource


def _error(status: int, code: str, message: str, **extra) -> JSONResponse:
    return JSONResponse(
        status_code=status, content={"error": {"code": code, "message": message}, **extra}
    )


def create_app(
    *,
    client: SimulationClient,
    source: MarketDataSource,
    store: ArtifactStore | None = None,
    missing: str = "warn",
) -> FastAPI:
    """Build the API app with its dependencies injected."""

    store = store or InMemoryArtifactStore()
    runs: dict[str, RunRecord] = {}
    app = FastAPI(title="Catalyst Backtest API", version="0.1.0")

    @app.get("/health")
    def health() -> dict:
        return {"status": "ok", "service": "catalyst-backtest-api"}

    @app.post("/backtests", status_code=201)
    def create_backtest(request: BacktestRequest):
        # Request validation is handled by FastAPI against the BacktestRequest
        # contract model (malformed bodies -> 422 automatically).
        record = run_backtest(request, client=client, source=source, store=store, missing=missing)
        runs[record.id] = record
        if not record.ok:
            return _error(422, "backtest_failed", record.error or "unknown error", id=record.id)
        return {"id": record.id, "status": record.status.value}

    @app.get("/backtests/{run_id}")
    def get_backtest(run_id: str) -> dict:
        record = runs.get(run_id)
        if record is None:
            raise HTTPException(status_code=404, detail=f"no backtest {run_id!r}")
        return {"id": record.id, "status": record.status.value, "error": record.error}

    @app.get("/backtests/{run_id}/result")
    def get_result(run_id: str):
        if run_id not in runs:
            raise HTTPException(status_code=404, detail=f"no backtest {run_id!r}")
        result = store.read_result(run_id)
        if result is None:
            return _error(
                409, "no_result", f"backtest {run_id!r} has no result (it may have failed)"
            )
        return result

    @app.get("/backtests/{run_id}/events")
    def get_events(run_id: str) -> dict:
        if run_id not in runs:
            raise HTTPException(status_code=404, detail=f"no backtest {run_id!r}")
        trace = store.read_trace(run_id)
        if trace is None:
            return {"events": []}
        return {"events": trace.get("events", [])}

    return app


__all__ = ["create_app"]
