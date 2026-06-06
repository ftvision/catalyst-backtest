"""Market data package for Catalyst backtesting.

Fetches, normalizes, and caches the historical data a compiled graph requires,
emitting a ``catalyst_contracts.MarketDataBundle`` the simulation engine reads.

The engine never fetches raw data; it only consumes the normalized bundle this
package produces. Network access is injected (see :mod:`live`), so bundles can be
built entirely offline from fixtures.
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
from .parquet_store import ParquetSource, ParquetStore
from .planner import MissingDataError, build_bundle
from .sources import FixtureSource, MarketDataSource

__version__ = "0.1.0"

__all__ = [
    "__version__",
    "build_bundle",
    "MissingDataError",
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
]
