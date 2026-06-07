"""Normalized market data bundle contract models (market-data-bundle.schema.json)."""

from __future__ import annotations

from datetime import datetime
from typing import Literal

from pydantic import Field

from ._base import Decimal, StrictModel
from .request import Interval


class Candle(StrictModel):
    ts: datetime
    open: Decimal
    high: Decimal
    low: Decimal
    close: Decimal
    volume: Decimal | None = None


class CandleSeries(StrictModel):
    venue: str
    symbol: str
    quote: str = "USD"
    points: list[Candle] = Field(default_factory=list)


class FundingPoint(StrictModel):
    ts: datetime
    rate: Decimal


class FundingSeries(StrictModel):
    venue: str
    symbol: str
    points: list[FundingPoint] = Field(default_factory=list)


class GasPoint(StrictModel):
    ts: datetime
    gas_usd: Decimal


class GasSeries(StrictModel):
    chain: str
    points: list[GasPoint] = Field(default_factory=list)


class YieldPoint(StrictModel):
    ts: datetime
    apr: Decimal


class YieldSeries(StrictModel):
    protocol: str
    asset: str
    chain: str
    pool: str | None = None
    points: list[YieldPoint] = Field(default_factory=list)


class Coverage(StrictModel):
    start: datetime | None = None
    end: datetime | None = None
    complete: bool | None = None


class Provider(StrictModel):
    name: str
    kind: Literal["candles", "funding", "gas", "yields"]
    coverage: Coverage | None = None
    provenance: Literal["native", "reference", "derived"] | None = None
    venue: str | None = None
    symbol: str | None = None


class MarketDataBundle(StrictModel):
    schema_version: str = "catalyst.backtest.market_data_bundle.v1"
    interval: Interval
    start: datetime
    end: datetime
    candles: list[CandleSeries] = Field(default_factory=list)
    funding: list[FundingSeries] = Field(default_factory=list)
    gas: list[GasSeries] = Field(default_factory=list)
    yields: list[YieldSeries] = Field(default_factory=list)
    providers: list[Provider] = Field(default_factory=list)
    warnings: list[str] = Field(default_factory=list)


__all__ = [
    "MarketDataBundle",
    "CandleSeries",
    "Candle",
    "FundingSeries",
    "GasSeries",
    "YieldSeries",
    "Provider",
    "Coverage",
]
