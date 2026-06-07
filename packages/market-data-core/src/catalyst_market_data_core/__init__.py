"""Shared core for Catalyst market-data ingestion.

Holds the durable Parquet **series store** (the cross-language storage contract
the Rust loader reads) and the injected HTTP **transport** seam. Each data vendor
is its own package (`catalyst-market-data` for Binance/Aave/gas,
`catalyst-market-data-dune`, `catalyst-market-data-bigquery`, …) and depends on
this core so the store layout and fetch plumbing are single-sourced.
"""

from __future__ import annotations

from .quality import CandleQualityReport, filter_candle_outliers
from .store import ParquetSource, ParquetStore
from .transport import Transport, http_transport, network_disabled

__version__ = "0.1.0"

__all__ = [
    "__version__",
    "ParquetStore",
    "ParquetSource",
    "Transport",
    "http_transport",
    "network_disabled",
    "filter_candle_outliers",
    "CandleQualityReport",
]
