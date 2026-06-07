"""Dune Analytics ingester for the Catalyst market-data store.

Authoring a query on Dune and passing its numeric ``query_id``, this package
executes it, polls for completion, and writes the rows into the Parquet store as
gas or candle series. Per [ADR 0001] this is ingestion only — the run path never
calls Dune.

[ADR 0001]: ../../../docs/adr/0001-language-boundary.md
"""

from __future__ import annotations

from .client import DuneClient
from .ingest import fetch_candles, fetch_gas, ingest_candles, ingest_gas, parse_ts

__version__ = "0.1.0"

__all__ = [
    "__version__",
    "DuneClient",
    "fetch_gas",
    "fetch_candles",
    "ingest_gas",
    "ingest_candles",
    "parse_ts",
]
