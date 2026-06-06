# catalyst-market-data-loader

Reads the **Parquet historical market-data store** directly into a
`catalyst_contracts::MarketDataBundle`, so a simulation can be driven by a
dataset *reference* + window instead of a bundle serialized over the wire
(issue #29). This is the Rust consumer of the store written by the Python
ingesters (issue #30); the layout is the cross-language contract in
[`docs/market-data-storage.md`](../../docs/market-data-storage.md).

## Why

The previous path serialized the whole bundle to JSON, POSTed it, and re-parsed
it in Rust — wasteful at fine granularity. Here Rust reads Parquet directly:

- **projection** — only the needed value columns,
- **partition pruning** — only the date files inside the window,
- **window filter** — only rows whose `ts` is in `[start, end]`,
- value columns are decimal-strings, so they map straight into the contract (no
  decimal conversion), and the engine stays a pure function of the bundle.

## Usage

`load_bundle` is **async** (object stores are async); the parquet decode is
synchronous over the fetched bytes.

```rust
use catalyst_market_data_loader::{load_bundle, BundleRef, DataRequirements, CandleReq};

let bundle = load_bundle(
    &BundleRef {
        // local path, file://, s3://bucket/prefix, or gs://... — same code path
        root: "data/market-data".into(),
        data_requirements: DataRequirements {
            candles: vec![CandleReq { venue: "base".into(), symbol: "ETH".into() }],
            ..Default::default()
        },
    },
    "2024-01-01T00:00:00Z", "2024-02-01T00:00:00Z", "1h",
).await?;
```

`DataRequirements` mirrors the Python compiler's output (it's *data*, not logic —
the caller passes which series to load; see #28 re: the trigger-logic duplication
that is tracked separately).

The simulation service consumes this: `POST /simulate` accepts a `market_data_ref`
(this `BundleRef`) instead of an inline `market_data` bundle.

## Storage backends

Access goes through [`object_store`], so `root` can be:

| `root` | Backend |
| --- | --- |
| `data/market-data` or `file:///abs/path` | local filesystem |
| `s3://bucket/prefix` | S3 (creds via standard env: `AWS_*`) |
| `gs://bucket/prefix` | Google Cloud Storage |

Reads prune by date partition and project columns, so only the needed objects
and rows are fetched.

## Scope / not yet

- CPU-bound parquet decode runs inline in the async fn (fine at current scale;
  could move to `spawn_blocking` if it ever competes with IO).
- Missing series produce warnings + `incomplete` coverage (matching the Python planner), not errors.

## Tests

```bash
cargo test -p catalyst-market-data-loader
```

Tests write a Parquet store mirroring the #30 layout, then assert window
filtering, partition pruning, missing-series warnings, and contract round-trip.
