"""Catalyst graph contract models (graph.schema.json).

The graph is the raw strategy input: ``signal`` and ``action`` nodes joined by
directed edges. ``Node.config`` is kept as a free-form mapping here so the
contract faithfully round-trips any producer payload; the typed ``*Config``
models below are provided for the graph compiler to validate against.
"""

from __future__ import annotations

from typing import Any, Literal

from pydantic import Field

from ._base import Decimal, OpenModel, StrictModel

NodeKind = Literal["action", "signal"]
NodeSubtype = Literal["swap", "perp_order", "yield_deposit", "yield_withdraw", "price_threshold"]
ThresholdOperator = Literal["<", "<=", ">", ">=", "==", "!="]
PerpSide = Literal["long", "short"]


class Node(StrictModel):
    id: str
    kind: NodeKind
    subtype: NodeSubtype
    config: dict[str, Any]
    enabled: bool = True


class Edge(StrictModel):
    from_: str = Field(alias="from")
    to: str

    model_config = {"populate_by_name": True, "extra": "forbid"}


class Graph(OpenModel):
    schema_version: str = "catalyst.graph.definition.v1"
    variables: dict[str, Any] = Field(default_factory=dict)
    settings: dict[str, Any] = Field(default_factory=dict)
    nodes: list[Node] = Field(default_factory=list)
    edges: list[Edge] = Field(default_factory=list)


# --- Typed config models (used by the graph compiler to normalize node.config) ---


class SwapConfig(StrictModel):
    from_asset: str
    to_asset: str
    amount: Decimal
    chain: str


class PerpOrderConfig(StrictModel):
    symbol: str
    side: PerpSide
    size_usd: Decimal
    leverage: Decimal | None = None
    chain: str
    order_type: Literal["market", "limit"] = "market"
    reduce_only: bool = False


class YieldConfig(StrictModel):
    chain: str
    protocol: str
    pool: str | None = None
    asset: str
    amount: Decimal


class PriceThresholdConfig(StrictModel):
    symbol: str
    operator: ThresholdOperator
    threshold: Decimal


__all__ = [
    "Node",
    "Edge",
    "Graph",
    "NodeKind",
    "NodeSubtype",
    "SwapConfig",
    "PerpOrderConfig",
    "YieldConfig",
    "PriceThresholdConfig",
]
