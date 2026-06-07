# Market-Data Storage Schema

The **historical series store** is the durable source of truth for deep history
(candles, funding, gas, yields). It sits *upstream* of the per-run `BundleCache`.

Per [ADR 0001](adr/0001-language-boundary.md), this store **is** the Python↔Rust
boundary: Python ingesters **write** it and the Rust loader **reads** it — the two
languages communicate through this data at rest, not through shared code. This doc
is therefore the cross-language storage contract; both sides must agree on it
exactly.

## Format & layout

Parquet with Hive-style partitioning, one file per series per UTC date:

```
<root>/candles/venue=<venue>/symbol=<symbol>/interval=<interval>/<YYYY-MM-DD>.parquet
<root>/funding/venue=<venue>/symbol=<symbol>/<YYYY-MM-DD>.parquet
<root>/gas/chain=<chain>/<YYYY-MM-DD>.parquet
<root>/yields/protocol=<p>/asset=<a>/chain=<c>/pool=<pool|_none>/<YYYY-MM-DD>.parquet
<root>/liquidity/venue=<venue>/symbol=<symbol>/<YYYY-MM-DD>.parquet
```

Partitioning enables **partition pruning** (read only the dates in the window)
and **projection** (read only needed columns).

## Columns

| Series | Columns |
| --- | --- |
| candles | `ts`, `open`, `high`, `low`, `close`, `volume` |
| funding | `ts`, `rate` |
| gas | `ts`, `gas_usd` |
| yields | `ts`, `apr` |
| liquidity | `ts`, `reserve_base`, `reserve_quote` |

- **`ts`** — `timestamp[us, tz=UTC]`.
- **Value columns** — stored as **strings** (decimal-as-string), matching the
  `catalyst_contracts` wire convention exactly, so no precision is lost and no
  conversion is needed when building a `MarketDataBundle`. A future revision may
  switch to Parquet `Decimal128`; that change must be made on both the writer
  and every reader at once.

## Conventions

- Writes **merge by `ts`** within a date file (idempotent incremental backfill).
- A reference price stored under a venue (e.g. Binance klines written under
  `venue=hyperliquid, symbol=ETH`) is an **approximation** of that venue's own
  mark — fine for directional backtests, not execution-grade. The real provenance
  is recorded in the `_provenance.json` sidecar (below).

## Sidecars (manifests at `<root>/`)

Small JSON files at the store root, keyed by `"<kind>/<key>"` (e.g.
`"candles/base/ETH/1h"`). They are metadata *about* the series, not series data:

- **`_provenance.json`** — per series, `native` (the venue's own price/feed),
  `reference` (a proxy stored under another venue, e.g. a CEX price), or
  `derived`. The Rust loader reads it to label provider metadata. Written via
  `ParquetStore.set_provenance` / read via `read_provenance`.
- **`_quality.json`** — per series, the ingestion-time cleaning report
  (`outliers_removed`, `wicks_repaired`, `method`, `affected_ranges`,
  `repaired_ranges`). Written via `set_quality` / read via `read_quality`. See
  [market-data-construction.md](market-data-construction.md).
- **`_validation.json`** — per series, the latest cross-reference QA report
  (per-field deviation vs an independent reference + OHLC invariants), written by
  `scripts/validate_market_data.py`. There is no `ParquetStore` accessor for this
  one yet; the script reads/writes it directly.

## Coverage

`ParquetStore.coverage(base)` (a series partition `Path`) returns `(min_ts, max_ts)` for a series so a
run can report what history is actually available and the missing-data policy
can act, rather than silently returning gaps.

## Where it lives

The layout is identical regardless of physical location:

- **dev / single machine** — local directory (e.g. `data/market-data/`).
- **shared / prod** — the same tree in object storage (S3 / R2 / GCS); Rust reads
  via `object_store`, Python via `pyarrow`/`duckdb`. Moving from local to a bucket
  is a URL/credentials change, not a code change.

See issue #30 (this store) and #29 (Rust reading it directly).
