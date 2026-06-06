"""Pure helpers for the workbench endpoints: graph hashing/summary and coverage."""

from __future__ import annotations

import hashlib
import json

from catalyst_contracts import Graph, MarketDataBundle
from catalyst_graph_compiler import CompiledGraph


def graph_hash(graph: Graph) -> str:
    """Stable short hash of a graph (canonical JSON), for run history grouping."""

    canonical = json.dumps(
        graph.model_dump(by_alias=True, exclude_none=True, mode="json"), sort_keys=True
    )
    return hashlib.sha256(canonical.encode()).hexdigest()[:12]


def graph_summary(graph: Graph, compiled: CompiledGraph) -> dict:
    """Node/edge counts and the enabled signal/action ids for the Run Setup view."""

    return {
        "node_count": len(graph.nodes),
        "edge_count": len(graph.edges),
        "signals": [s.id for s in compiled.signals],
        "actions": [a.id for a in compiled.actions],
    }


def _series_rows(bundle: MarketDataBundle) -> list[dict]:
    """One coverage row per series in the bundle."""

    rows: list[dict] = []

    def row(kind: str, key: dict, points: list) -> dict:
        return {
            "kind": kind,
            **key,
            "points": len(points),
            "complete": len(points) > 0,
            "start": points[0].ts.isoformat() if points else None,
            "end": points[-1].ts.isoformat() if points else None,
        }

    for s in bundle.candles:
        rows.append(row("candles", {"venue": s.venue, "symbol": s.symbol}, s.points))
    for s in bundle.funding:
        rows.append(row("funding", {"venue": s.venue, "symbol": s.symbol}, s.points))
    for s in bundle.gas:
        rows.append(row("gas", {"chain": s.chain}, s.points))
    for s in bundle.yields:
        rows.append(
            row("yields", {"protocol": s.protocol, "asset": s.asset, "chain": s.chain}, s.points)
        )
    return rows


def coverage_response(bundle: MarketDataBundle) -> dict:
    """Per-series coverage + provider metadata + warnings for the coverage view."""

    return {
        "coverage": _series_rows(bundle),
        "providers": [p.model_dump(exclude_none=True) for p in bundle.providers],
        "warnings": list(bundle.warnings),
    }


__all__ = ["graph_hash", "graph_summary", "coverage_response"]
