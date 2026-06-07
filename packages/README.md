# Python Packages

Python packages are managed with `uv`.

Per [ADR 0001](../docs/adr/0001-language-boundary.md) the deterministic run path
(compile → run → summarize) is Rust. Python's role is **data ingestion** and
**analysis**; the boundary between the two is the Parquet store and the Rust
service's HTTP API. The run-path packages (`graph-compiler`, `result-reporter`,
`backtest-worker`, `backtest-api`) were retired once their Rust equivalents
landed.

Packages:

- `contracts` — Pydantic models generated from the shared JSON Schemas
  (`schemas/`); the cross-language data contract.
- `market-data-core` — shared ingestion plumbing: the Parquet **series store**
  (the storage contract the Rust loader reads) + the injected HTTP transport seam.
- `market-data` — ingestion vendor: Binance klines, DefiLlama (Aave yields), EVM
  gas. Writes the store via `market-data-core`.
- `market-data-dune` — ingestion vendor: Dune Analytics saved queries
  (`catalyst-ingest-dune`).
- `market-data-bigquery` — ingestion vendor: Google BigQuery public crypto
  datasets (`catalyst-ingest-bigquery`).

Each market-data **vendor** is its own package (its own CLI + credentials), all
sharing `market-data-core` so the store format and fetch seam are single-sourced.
We can cross-reference the same series (e.g. gas) ingested from different vendors.
