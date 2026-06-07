"""Google BigQuery ingester for the Catalyst market-data store.

Reads Google's public crypto datasets (e.g. `bigquery-public-data.crypto_ethereum`)
and writes the Parquet store. Built-in support for historical L1 gas from the
blocks table; candles are bring-your-own-SQL (the public Ethereum dataset has no
curated USD price feed). Per [ADR 0001] this is ingestion only.

[ADR 0001]: ../../../docs/adr/0001-language-boundary.md
"""

from __future__ import annotations

from .gas import fetch_gas, gas_sql, ingest_gas
from .prices import fetch_candles, ingest_candles
from .runner import QueryRunner, bigquery_runner

__version__ = "0.1.0"

__all__ = [
    "__version__",
    "QueryRunner",
    "bigquery_runner",
    "gas_sql",
    "fetch_gas",
    "ingest_gas",
    "fetch_candles",
    "ingest_candles",
]
