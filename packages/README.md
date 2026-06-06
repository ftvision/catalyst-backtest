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
- `market-data` — ingestion: fetch from sources (Binance, DefiLlama, EVM gas,
  Hyperliquid) and write the Parquet store the Rust service reads.
