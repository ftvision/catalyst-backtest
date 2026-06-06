"""Market data source abstraction and a network-free fixture source.

A :class:`MarketDataSource` knows how to return normalized series for the four
data kinds: candles, funding, gas, and yields. Ingesters and offline tests share
this shape so a fixture, a live fetcher, or the Parquet store are interchangeable.

:class:`FixtureSource` serves pre-baked series from memory or a directory of
``MarketDataBundle`` JSON files, for entirely offline use.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Protocol, runtime_checkable

from catalyst_contracts import (
    Candle,
    CandleSeries,
    FundingSeries,
    GasSeries,
    MarketDataBundle,
    YieldSeries,
)
from catalyst_contracts.market_data import FundingPoint, GasPoint, YieldPoint


@runtime_checkable
class MarketDataSource(Protocol):
    """Returns normalized series for a (venue/chain, symbol/asset) over a range.

    Implementations return an empty list when they have no data for a request.
    """

    name: str

    def candles(self, venue: str, symbol: str) -> list[Candle]: ...

    def funding(self, venue: str, symbol: str) -> list[FundingPoint]: ...

    def gas(self, chain: str) -> list[GasPoint]: ...

    def yields(
        self, protocol: str, asset: str, chain: str, pool: str | None
    ) -> list[YieldPoint]: ...


class FixtureSource:
    """A fully offline source backed by an in-memory ``MarketDataBundle``.

    Build one from a bundle object, a dict, or a JSON file on disk. Lookups are
    keyed the same way the contract bundle is, so this is the source used by
    tests and deterministic fixture-backed runs.
    """

    name = "fixture"

    def __init__(self, bundle: MarketDataBundle) -> None:
        self._bundle = bundle
        self._candles = {(s.venue, s.symbol): s.points for s in bundle.candles}
        self._funding = {(s.venue, s.symbol): s.points for s in bundle.funding}
        self._gas = {s.chain: s.points for s in bundle.gas}
        self._yields = {(s.protocol, s.asset, s.chain, s.pool): s.points for s in bundle.yields}

    @classmethod
    def from_dict(cls, data: dict) -> FixtureSource:
        return cls(MarketDataBundle.model_validate(data))

    @classmethod
    def from_file(cls, path: str | Path) -> FixtureSource:
        return cls.from_dict(json.loads(Path(path).read_text()))

    def candles(self, venue: str, symbol: str) -> list[Candle]:
        return list(self._candles.get((venue, symbol), []))

    def funding(self, venue: str, symbol: str) -> list[FundingPoint]:
        return list(self._funding.get((venue, symbol), []))

    def gas(self, chain: str) -> list[GasPoint]:
        return list(self._gas.get(chain, []))

    def yields(self, protocol: str, asset: str, chain: str, pool: str | None) -> list[YieldPoint]:
        return list(self._yields.get((protocol, asset, chain, pool), []))


__all__ = [
    "MarketDataSource",
    "FixtureSource",
    "CandleSeries",
    "FundingSeries",
    "GasSeries",
    "YieldSeries",
]
