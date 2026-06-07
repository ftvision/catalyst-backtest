# catalyst-market-data-core

Shared plumbing for Catalyst's market-data **ingestion** packages. Per
[ADR 0001](../../docs/adr/0001-language-boundary.md) ingestion is Python; each
data vendor is its own package and depends on this core so the store format and
fetch seam are single-sourced (no drift across vendors).

## What's here

- **`ParquetStore` / `ParquetSource`** — the durable, partitioned Parquet *series
  store* (candles / funding / gas / yields). Writes merge by timestamp; reads
  prune by date partition and filter by window. This is the **cross-language
  storage contract** the Rust loader reads directly (issue #29); the layout is
  documented in [docs/market-data-storage.md](../../docs/market-data-storage.md).
- **`Transport` / `http_transport()` / `network_disabled`** — the injected HTTP
  seam for REST vendors. A `Transport` is one callable covering GET and POST:

  ```python
  transport(method, url, *, headers=None, params=None, json=None) -> parsed JSON
  ```

  Ingesters take a `Transport` so fetching is testable offline; a real one is
  built lazily from `httpx`. The default (`network_disabled`) refuses calls so a
  misconfigured ingester fails loudly.

## Vendors built on this

| Package | Vendor |
| --- | --- |
| `catalyst-market-data` | Binance klines, DefiLlama (Aave), EVM gas |
| `catalyst-market-data-dune` | Dune Analytics saved queries |
| `catalyst-market-data-bigquery` | Google BigQuery public crypto datasets |

## Tests

```bash
uv run pytest packages/market-data-core
```

Store round-trip + window/partition reads, merge-by-ts, and the transport seam.
