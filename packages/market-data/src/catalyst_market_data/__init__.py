"""Market data package for Catalyst backtesting.

Per [ADR 0001] this package is the **ingestion** side of the system: it fetches
historical data from external sources (Binance, DefiLlama, EVM gas, Hyperliquid)
and writes it to the Parquet store. The Rust service reads that store directly
(`catalyst-market-data-loader`); the run path no longer touches Python.

Network access is injected (see :mod:`live`), so fetchers run entirely offline
against fixtures/fake transports in tests.

[ADR 0001]: ../../../docs/adr/0001-language-boundary.md
"""

from __future__ import annotations

from .cache import DEFAULT_CACHE_ROOT, BundleCache, bundle_key
from .live import (
    CallableGasSource,
    CallableYieldSource,
    CompositeSource,
    HyperliquidSource,
    NetworkDisabledTransport,
    Transport,
)
from .binance import fetch_klines, httpx_transport, ingest_binance
from .defillama import fetch_aave_yields, ingest_aave_yields
from .evm_gas import (
    constant_gas_series,
    fetch_recent_gas,
    httpx_rpc_transport,
    ingest_constant_gas,
    ingest_recent_gas,
)
from .parquet_store import ParquetSource, ParquetStore
from .sources import FixtureSource, MarketDataSource

__version__ = "0.1.0"

__all__ = [
    "__version__",
    "MarketDataSource",
    "FixtureSource",
    "HyperliquidSource",
    "CallableGasSource",
    "CallableYieldSource",
    "CompositeSource",
    "Transport",
    "NetworkDisabledTransport",
    "BundleCache",
    "bundle_key",
    "DEFAULT_CACHE_ROOT",
    # historical store (#30)
    "ParquetStore",
    "ParquetSource",
    "fetch_klines",
    "ingest_binance",
    "httpx_transport",
    # additional sources (#30)
    "fetch_aave_yields",
    "ingest_aave_yields",
    "fetch_recent_gas",
    "constant_gas_series",
    "ingest_recent_gas",
    "ingest_constant_gas",
    "httpx_rpc_transport",
]
