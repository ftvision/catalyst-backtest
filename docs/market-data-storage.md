# Market-Data Storage Schema

The **historical series store** is the durable source of truth for deep history
(candles, funding, gas, yields). It sits *upstream* of the per-run `BundleCache`.
This doc is the cross-language storage contract: both the Python ingesters /
`ParquetSource` and the future Rust loader (issue #29) read this exact layout.

## Format & layout

Parquet with Hive-style partitioning, one file per series per UTC date:

```
<root>/candles/venue=<venue>/symbol=<symbol>/interval=<interval>/<YYYY-MM-DD>.parquet
<root>/funding/venue=<venue>/symbol=<symbol>/<YYYY-MM-DD>.parquet
<root>/gas/chain=<chain>/<YYYY-MM-DD>.parquet
<root>/yields/protocol=<p>/asset=<a>/chain=<c>/pool=<pool|_none>/<YYYY-MM-DD>.parquet
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
  mark — fine for directional backtests, not execution-grade. Record the real
  provenance in a coverage/manifest entry when it matters.

## Coverage

`ParquetStore.coverage(series_dir)` returns `(min_ts, max_ts)` for a series so a
run can report what history is actually available and the missing-data policy
can act, rather than silently returning gaps.

## Where it lives

The layout is identical regardless of physical location:

- **dev / single machine** — local directory (e.g. `data/market-data/`).
- **shared / prod** — the same tree in object storage (S3 / R2 / GCS); Rust reads
  via `object_store`, Python via `pyarrow`/`duckdb`. Moving from local to a bucket
  is a URL/credentials change, not a code change.

See issue #30 (this store) and #29 (Rust reading it directly).
