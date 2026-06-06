"""Turn a compiled graph's data requirements into a normalized bundle.

The planner is the integration point between the graph compiler and a market
data source: it reads ``CompiledGraph.data_requirements`` and asks the source for
exactly those series, then assembles a ``MarketDataBundle`` with provider
metadata, per-series coverage, and warnings for anything missing.

Missing-data handling is *explicit and policy-compatible*: the planner never
silently drops a required series. With ``missing="warn"`` (default) an empty
required series becomes a warning and an ``incomplete`` coverage flag; with
``missing="fail"`` it raises :class:`MissingDataError`. The simulation policy's
``data.missing_required`` decides which to use.
"""

from __future__ import annotations

from datetime import datetime
from typing import Literal

from catalyst_contracts import (
    CandleSeries,
    FundingSeries,
    GasSeries,
    MarketDataBundle,
    Provider,
    YieldSeries,
)
from catalyst_contracts.market_data import Coverage
from catalyst_graph_compiler import CompiledGraph

from .sources import MarketDataSource

MissingPolicy = Literal["warn", "fail"]


class MissingDataError(RuntimeError):
    """A required data series was unavailable and the policy is ``fail``."""


def _iso(ts: datetime) -> str:
    return ts.strftime("%Y-%m-%dT%H:%M:%SZ")


def build_bundle(
    compiled: CompiledGraph,
    *,
    start: datetime,
    end: datetime,
    interval: str,
    source: MarketDataSource,
    missing: MissingPolicy = "warn",
) -> MarketDataBundle:
    """Build a normalized :class:`MarketDataBundle` for ``compiled``."""

    reqs = compiled.data_requirements
    warnings: list[str] = []
    providers: list[Provider] = []
    source_name = getattr(source, "name", "source")
    coverage = Coverage(start=_iso(start), end=_iso(end), complete=True)

    def note_missing(description: str) -> None:
        message = f"no {description} from {source_name!r}"
        if missing == "fail":
            raise MissingDataError(message)
        warnings.append(message)

    candle_series: list[CandleSeries] = []
    for req in reqs.candles:
        points = source.candles(req.venue, req.symbol)
        if not points:
            note_missing(f"candles for {req.symbol} on {req.venue}")
        candle_series.append(CandleSeries(venue=req.venue, symbol=req.symbol, points=points))
    if reqs.candles:
        providers.append(_provider(source_name, "candles", candle_series, coverage))

    funding_series: list[FundingSeries] = []
    for req in reqs.funding:
        points = source.funding(req.venue, req.symbol)
        if not points:
            note_missing(f"funding for {req.symbol} on {req.venue}")
        funding_series.append(FundingSeries(venue=req.venue, symbol=req.symbol, points=points))
    if reqs.funding:
        providers.append(_provider(source_name, "funding", funding_series, coverage))

    gas_series: list[GasSeries] = []
    for req in reqs.gas:
        points = source.gas(req.chain)
        if not points:
            note_missing(f"gas for {req.chain}")
        gas_series.append(GasSeries(chain=req.chain, points=points))
    if reqs.gas:
        providers.append(_provider(source_name, "gas", gas_series, coverage))

    yield_series: list[YieldSeries] = []
    for req in reqs.yields:
        points = source.yields(req.protocol, req.asset, req.chain, req.pool)
        if not points:
            note_missing(f"yields for {req.protocol}/{req.asset} on {req.chain}")
        yield_series.append(
            YieldSeries(
                protocol=req.protocol,
                asset=req.asset,
                chain=req.chain,
                pool=req.pool,
                points=points,
            )
        )
    if reqs.yields:
        providers.append(_provider(source_name, "yields", yield_series, coverage))

    return MarketDataBundle(
        interval=interval,
        start=start,
        end=end,
        candles=candle_series,
        funding=funding_series,
        gas=gas_series,
        yields=yield_series,
        providers=providers,
        warnings=warnings,
    )


def _provider(name: str, kind: str, series, base_coverage: Coverage) -> Provider:
    complete = all(getattr(s, "points", []) for s in series) if series else False
    return Provider(
        name=name,
        kind=kind,
        coverage=Coverage(start=base_coverage.start, end=base_coverage.end, complete=complete),
    )


__all__ = ["build_bundle", "MissingDataError", "MissingPolicy"]
