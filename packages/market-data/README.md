# catalyst-market-data

The **ingestion** side of Catalyst. Per
[ADR 0001](../../docs/adr/0001-language-boundary.md) the deterministic run path is
Rust; this package fetches historical data from external sources and writes it to
the **Parquet store** that the Rust service reads (`catalyst-market-data-loader`).
Python no longer assembles bundles for the engine — it only fills the store.

Network access is always **injected**. The default transport
(`NetworkDisabledTransport`) refuses to make calls, so fetchers run entirely
offline against fixtures / fake transports in tests.

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

# Read them back (e.g. for analysis):
src = ParquetSource("data/market-data", start, end, "1h")
candles = src.candles("hyperliquid", "ETH")
```

The Rust service reads the same store directly at run time (issue #29).

### Ingesters / CLI

| Source | What | CLI |
| --- | --- | --- |
| **Binance klines** | candles (free, deep, keyless reference price) | `ingest-binance` |
| **DefiLlama Aave APY** | yields (free, historical; APY% → APR fraction) | `ingest-aave-yields` |
| **EVM gas** | per-chain gas: recent `eth_feeHistory` (real, recent-only) or a flat estimate over a window | `ingest-gas` |

```bash
# candles (Binance reference)
python -m catalyst_market_data.cli ingest-binance \
  --root data/market-data --venue hyperliquid --symbol ETH \
  --binance-symbol ETHUSDT --interval 1h \
  --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z

# Aave yields (DefiLlama pool UUID from yields.llama.fi/pools)
python -m catalyst_market_data.cli ingest-aave-yields \
  --root data/market-data --asset USDC --chain base --pool usdc \
  --pool-id <defillama-pool-uuid> \
  --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z

# gas: a flat estimate over a window (deep history isn't available free)
python -m catalyst_market_data.cli ingest-gas \
  --root data/market-data --chain base --constant 0.02 --interval 1h \
  --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z
```

> **Gas caveat:** free historical gas isn't available from RPC (`eth_feeHistory`
> is recent-only). `ingest-gas` here offers a *real recent* RPC mode and a
> *flat-estimate* backfill for historical windows. For **deep historical gas**,
> use the dedicated vendor packages: `catalyst-market-data-bigquery` (L1 from the
> public Ethereum `blocks` table) or `catalyst-market-data-dune` (any chain a
> query covers) — you can cross-reference them against each other.

## Fetch sources

Ingesters fetch through small source/transport abstractions that normalize to the
`catalyst_contracts` series types before writing the store:

| Source | Role |
| --- | --- |
| `HyperliquidSource` | Real Hyperliquid `info` API candles + funding; HTTP is **injected** via a `Transport`. |
| `CallableGasSource` / `CallableYieldSource` | Normalize an injected fetch callable (Base RPC gas, Aave yields). |
| `CompositeSource` | Routes each kind to a dedicated source. |
| `FixtureSource` | Fully offline; serves a pre-baked bundle (used by tests). |
| `ParquetSource` | Reads the historical Parquet store back as normalized series. |

## Cache

`BundleCache` reads/writes `MarketDataBundle` JSON under a cache root
(`data/market-data/` by default), keyed by `bundle_key(...)` — a stable hash of
range + interval + requirements. Useful for local analysis workflows.

## Tests

```bash
uv run pytest packages/market-data
```

All tests are network-free: Parquet store round-trips + window/partition reads,
the Binance/Aave/gas ingesters via fake transports, Hyperliquid request
building/parsing, composite routing, source lookups, and cache round-trips.
