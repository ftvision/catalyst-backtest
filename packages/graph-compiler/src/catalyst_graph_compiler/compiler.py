"""Compile a raw Catalyst graph into a normalized :class:`CompiledGraph`.

Pipeline:

1. parse + structurally validate the graph (via ``catalyst_contracts.Graph``)
2. reject duplicate ids; drop disabled nodes (and edges touching them)
3. validate every enabled node's config against its typed config model
4. validate edge references
5. derive triggers (initial / signal-driven / action-chained) and signal targets
6. extract data requirements for the market-data package

The output is deterministic and serializable.
"""

from __future__ import annotations

from typing import Any

from catalyst_contracts import (
    Graph,
    PerpOrderConfig,
    PriceThresholdConfig,
    SwapConfig,
    YieldConfig,
)
from pydantic import ValidationError

from .errors import CompileError
from .model import (
    CandleRequirement,
    CompiledAction,
    CompiledGraph,
    CompiledSignal,
    DataRequirements,
    FundingRequirement,
    GasRequirement,
    Trigger,
    YieldRequirement,
)

# Assets treated as cash/quote (no price series needed to value them in USD).
STABLE_ASSETS = frozenset({"USDC", "USDT", "USD", "DAI", "USDC.E"})

# Fallback price feed for signals/symbols not traded on a single explicit venue.
DEFAULT_PRICE_VENUE = "hyperliquid"

_ACTION_SUBTYPES = {"swap", "perp_order", "yield_deposit", "yield_withdraw"}
_SIGNAL_SUBTYPES = {"price_threshold"}


def compile_graph(graph: Graph | dict[str, Any]) -> CompiledGraph:
    """Compile ``graph`` (a ``Graph`` model or raw dict) into a ``CompiledGraph``."""

    if not isinstance(graph, Graph):
        try:
            graph = Graph.model_validate(graph)
        except ValidationError as exc:  # pragma: no cover - exercised via tests
            raise CompileError(f"graph failed structural validation: {exc}") from exc

    warnings: list[str] = []

    if not graph.nodes:
        raise CompileError("graph has no nodes")

    # --- duplicate ids ---
    seen: set[str] = set()
    for node in graph.nodes:
        if node.id in seen:
            raise CompileError("duplicate node id", node_id=node.id)
        seen.add(node.id)

    all_ids = {node.id for node in graph.nodes}
    enabled = {node.id: node for node in graph.nodes if node.enabled}
    for node in graph.nodes:
        if not node.enabled:
            warnings.append(f"node {node.id!r} is disabled and was excluded")

    # --- validate enabled node kind/subtype + config ---
    typed_config: dict[str, Any] = {}
    for node in graph.nodes:
        if node.id not in enabled:
            continue
        typed_config[node.id] = _validate_node(node)

    # --- validate + filter edges ---
    edges: list[tuple[str, str]] = []
    for edge in graph.edges:
        if edge.from_ not in all_ids:
            raise CompileError(f"edge references unknown source node {edge.from_!r}")
        if edge.to not in all_ids:
            raise CompileError(f"edge references unknown target node {edge.to!r}")
        if edge.from_ not in enabled or edge.to not in enabled:
            warnings.append(
                f"edge {edge.from_!r} -> {edge.to!r} touches a disabled node and was dropped"
            )
            continue
        if enabled[edge.to].kind == "signal":
            warnings.append(
                f"edge {edge.from_!r} -> {edge.to!r} targets a signal; signals are evaluated "
                "by their own threshold, so this edge has no effect"
            )
            continue
        edges.append((edge.from_, edge.to))

    # --- per-symbol traded venue (for resolving signal price feeds) ---
    symbol_venues = _symbol_venue_map(enabled, typed_config)

    # --- build actions with triggers ---
    actions: list[CompiledAction] = []
    for node in graph.nodes:
        if node.id not in enabled or enabled[node.id].kind != "action":
            continue
        incoming = [src for (src, dst) in edges if dst == node.id]
        triggers: list[Trigger] = []
        if not incoming:
            triggers.append(Trigger(type="initial"))
        else:
            for src in incoming:
                src_kind = enabled[src].kind
                triggers.append(
                    Trigger(type="signal" if src_kind == "signal" else "action", source_id=src)
                )
        actions.append(
            CompiledAction(
                id=node.id,
                subtype=node.subtype,
                config=typed_config[node.id],
                triggers=triggers,
            )
        )

    # --- build signals with targets ---
    signals: list[CompiledSignal] = []
    for node in graph.nodes:
        if node.id not in enabled or enabled[node.id].kind != "signal":
            continue
        targets = [dst for (src, dst) in edges if src == node.id]
        if not targets:
            warnings.append(f"signal {node.id!r} has no downstream actions")
        signals.append(
            CompiledSignal(
                id=node.id,
                subtype=node.subtype,
                config=typed_config[node.id],
                targets=targets,
            )
        )

    data_requirements = _data_requirements(enabled, typed_config, symbol_venues)

    return CompiledGraph(
        actions=actions,
        signals=signals,
        data_requirements=data_requirements,
        warnings=warnings,
    )


def _validate_node(node) -> dict[str, Any]:
    """Validate a node's kind/subtype/config; return the normalized config dict."""

    if node.kind == "action" and node.subtype not in _ACTION_SUBTYPES:
        raise CompileError(f"unsupported action subtype {node.subtype!r}", node_id=node.id)
    if node.kind == "signal" and node.subtype not in _SIGNAL_SUBTYPES:
        raise CompileError(f"unsupported signal subtype {node.subtype!r}", node_id=node.id)

    model = {
        "swap": SwapConfig,
        "perp_order": PerpOrderConfig,
        "yield_deposit": YieldConfig,
        "yield_withdraw": YieldConfig,
        "price_threshold": PriceThresholdConfig,
    }[node.subtype]

    try:
        return model.model_validate(node.config).model_dump(exclude_none=True)
    except ValidationError as exc:
        raise CompileError(f"invalid {node.subtype} config: {exc}", node_id=node.id) from exc


def _symbol_venue_map(enabled, typed_config) -> dict[str, str]:
    """Map each traded non-stable symbol to its venue when unambiguous."""

    venues: dict[str, set[str]] = {}
    for node_id, node in enabled.items():
        cfg = typed_config[node_id]
        if node.subtype == "swap":
            venue = cfg["chain"]
            for asset in (cfg["from_asset"], cfg["to_asset"]):
                if asset.upper() not in STABLE_ASSETS:
                    venues.setdefault(asset, set()).add(venue)
        elif node.subtype == "perp_order":
            venues.setdefault(cfg["symbol"], set()).add(cfg["chain"])
    return {sym: next(iter(vs)) for sym, vs in venues.items() if len(vs) == 1}


def _data_requirements(enabled, typed_config, symbol_venues) -> DataRequirements:
    candles: dict[tuple[str, str], CandleRequirement] = {}
    funding: dict[tuple[str, str], FundingRequirement] = {}
    gas: dict[str, GasRequirement] = {}
    yields: dict[tuple[str, str, str, str | None], YieldRequirement] = {}

    def add_candle(venue: str, symbol: str) -> None:
        candles[(venue, symbol)] = CandleRequirement(venue=venue, symbol=symbol)

    for node_id, node in enabled.items():
        cfg = typed_config[node_id]
        if node.subtype == "swap":
            venue = cfg["chain"]
            for asset in (cfg["from_asset"], cfg["to_asset"]):
                if asset.upper() not in STABLE_ASSETS:
                    add_candle(venue, asset)
            if venue != "hyperliquid":
                gas[venue] = GasRequirement(chain=venue)
        elif node.subtype == "perp_order":
            venue = cfg["chain"]
            add_candle(venue, cfg["symbol"])
            funding[(venue, cfg["symbol"])] = FundingRequirement(venue=venue, symbol=cfg["symbol"])
        elif node.subtype in ("yield_deposit", "yield_withdraw"):
            key = (cfg["protocol"], cfg["asset"], cfg["chain"], cfg.get("pool"))
            yields[key] = YieldRequirement(
                protocol=cfg["protocol"],
                asset=cfg["asset"],
                chain=cfg["chain"],
                pool=cfg.get("pool"),
            )
            if cfg["chain"] != "hyperliquid":
                gas[cfg["chain"]] = GasRequirement(chain=cfg["chain"])
        elif node.subtype == "price_threshold":
            venue = symbol_venues.get(cfg["symbol"], DEFAULT_PRICE_VENUE)
            add_candle(venue, cfg["symbol"])

    return DataRequirements(
        candles=[candles[k] for k in sorted(candles)],
        funding=[funding[k] for k in sorted(funding)],
        gas=[gas[k] for k in sorted(gas)],
        yields=[yields[k] for k in sorted(yields, key=lambda k: tuple(str(p) for p in k))],
    )


__all__ = ["compile_graph", "STABLE_ASSETS", "DEFAULT_PRICE_VENUE"]
