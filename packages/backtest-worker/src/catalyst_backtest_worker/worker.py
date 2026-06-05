"""Coordinate a full backtest run.

Pipeline: validate + compile the graph, plan and build the market data bundle,
call the Rust simulation service, persist the raw trace, summarize it into a
result, and persist that separately. Errors at any stage are captured into the
returned :class:`RunRecord` rather than raised.
"""

from __future__ import annotations

import uuid
from dataclasses import dataclass, field
from enum import Enum

from catalyst_contracts import BacktestRequest
from catalyst_graph_compiler import CompileError, compile_graph
from catalyst_market_data import MissingDataError, build_bundle
from catalyst_market_data.sources import MarketDataSource
from catalyst_result_reporter import summarize

from .artifacts import ArtifactStore, InMemoryArtifactStore
from .client import SimulationClient


class RunStatus(str, Enum):
    PENDING = "pending"
    RUNNING = "running"
    SUCCEEDED = "succeeded"
    FAILED = "failed"


@dataclass
class RunRecord:
    """Outcome of a backtest run."""

    id: str
    status: RunStatus
    error: str | None = None
    trace_ref: str | None = None
    result_ref: str | None = None
    metadata_ref: str | None = None
    result: dict | None = None
    warnings: list[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return self.status is RunStatus.SUCCEEDED


def run_backtest(
    request: BacktestRequest | dict,
    *,
    client: SimulationClient,
    source: MarketDataSource,
    store: ArtifactStore | None = None,
    run_id: str | None = None,
    missing: str = "warn",
) -> RunRecord:
    """Run a backtest end to end, persisting raw trace and summarized result."""

    store = store or InMemoryArtifactStore()
    run_id = run_id or uuid.uuid4().hex
    if not isinstance(request, BacktestRequest):
        request = BacktestRequest.model_validate(request)

    try:
        compiled = compile_graph(request.graph)

        bundle = build_bundle(
            compiled,
            start=request.config.start,
            end=request.config.end,
            interval=request.config.interval,
            source=source,
            missing=missing,
        )

        graph_dict = request.graph.model_dump(by_alias=True, exclude_none=True, mode="json")
        config_dict = request.config.model_dump(mode="json", exclude_none=True)
        policy_dict = request.policy.model_dump(mode="json")

        trace = client.simulate(
            graph=graph_dict,
            config=config_dict,
            policy=policy_dict,
            market_data=bundle,
        )
    except (CompileError, MissingDataError) as exc:
        return RunRecord(id=run_id, status=RunStatus.FAILED, error=str(exc))
    except Exception as exc:  # noqa: BLE001 - surface any orchestration failure as a failed run
        return RunRecord(id=run_id, status=RunStatus.FAILED, error=f"{type(exc).__name__}: {exc}")

    # Persist the raw trace first, then the compact summarized result, separately.
    trace_json = trace.model_dump(mode="json", exclude_none=True)
    trace_ref = store.write_trace(run_id, trace_json)

    providers = [p.model_dump(mode="json", exclude_none=True) for p in bundle.providers]
    result = summarize(trace, data_coverage=providers)
    result_json = result.model_dump(by_alias=True, mode="json", exclude_none=True)
    result_ref = store.write_result(run_id, result_json)

    metadata = {
        "run_id": run_id,
        "policy": trace.policy.model_dump(by_alias=True, mode="json"),
        "providers": providers,
        "interval": request.config.interval,
        "start": request.config.start.isoformat(),
        "end": request.config.end.isoformat(),
        "warnings": list(bundle.warnings) + list(trace.warnings),
    }
    metadata_ref = store.write_metadata(run_id, metadata)

    return RunRecord(
        id=run_id,
        status=RunStatus.SUCCEEDED,
        trace_ref=trace_ref,
        result_ref=result_ref,
        metadata_ref=metadata_ref,
        result=result_json,
        warnings=metadata["warnings"],
    )


__all__ = ["RunStatus", "RunRecord", "run_backtest"]
