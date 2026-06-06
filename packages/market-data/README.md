# catalyst-market-data

Fetches, normalizes, and caches the historical data a compiled graph needs, and
emits a `catalyst_contracts.MarketDataBundle` for the simulation engine. The
engine never fetches raw data — it only reads the bundle this package produces.

## Pipeline

```python
from catalyst_graph_compiler import compile_graph
from catalyst_market_data import build_bundle, FixtureSource

compiled = compile_graph(raw_graph)
bundle = build_bundle(
    compiled,
    start=start, end=end, interval="1h",
    source=FixtureSource.from_file("eth_2h.json"),  # offline
)
```

`build_bundle` reads `compiled.data_requirements` and asks the source for exactly
those candle / funding / gas / yield series, then records provider metadata,
per-series coverage, and warnings.

## Sources

A `MarketDataSource` returns normalized series for the four data kinds. Provided
implementations:

| Source | Role |
| --- | --- |
| `FixtureSource` | Fully offline; serves a pre-baked bundle (used by tests and deterministic runs). |
| `HyperliquidSource` | Real Hyperliquid `info` API candles + funding; HTTP is **injected** via a `Transport`. |
| `CallableGasSource` / `CallableYieldSource` | Thin abstractions normalizing an injected fetch callable (Base RPC gas, Aave subgraph yields). |
| `CompositeSource` | Routes each kind to a dedicated source (HL candles/funding, EVM gas, Aave yields). |
| `ParquetSource` | Reads the **historical Parquet store** for deep history (see below). |

Network access is always injected. The default transport (`NetworkDisabledTransport`)
refuses to make calls, so fixture-backed runs are guaranteed offline.

## Historical store (deep history)

Live APIs are retention-limited (e.g. Hyperliquid `candleSnapshot` only serves
recent windows). The durable **series store** holds deep history as partitioned
Parquet; see [docs/market-data-storage.md](../../docs/market-data-storage.md) for
the layout/columns (the cross-language storage contract).

```python
from catalyst_market_data import ParquetStore, ParquetSource, ingest_binance, httpx_transport

# Backfill ETH candles from Binance klines (free, deep, keyless):
store = ParquetStore("data/market-data")
ingest_binance(store, venue="hyperliquid", symbol="ETH", binance_symbol="ETHUSDT",
               interval="1h", start=start, end=end, transport=httpx_transport())

# Read them back as a MarketDataSource:
src = ParquetSource("data/market-data", start, end, "1h")
```

Or via the CLI:

```bash
python -m catalyst_market_data.cli ingest-binance \
  --root data/market-data --venue hyperliquid --symbol ETH \
  --binance-symbol ETHUSDT --interval 1h \
  --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z
```

`ParquetSource` plugs into `build_bundle` like any other source — the engine is
unchanged. (Interim: Python reads Parquet → bundle. The end state is the Rust
loader reading the store directly, issue #29.)

## Missing-data handling

Explicit and policy-compatible — the planner never silently drops a required
series:

- `missing="warn"` (default): empty required series → warning + `incomplete`
  coverage flag.
- `missing="fail"`: empty required series → `MissingDataError`.

The simulation policy's `data.missing_required` selects which to use.

## Cache

`BundleCache` reads/writes `MarketDataBundle` JSON under a cache root
(`data/market-data/` by default), keyed by `bundle_key(...)` — a stable hash of
range + interval + requirements.

## Tests

```bash
uv run pytest packages/market-data
```

All tests are network-free: bundle assembly per graph family, missing-data
behavior, Hyperliquid request building/parsing via a fake transport, composite
routing, and cache round-trips.
