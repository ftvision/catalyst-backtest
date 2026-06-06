# Catalyst Backtest

Foundation repo for a graph-driven backtesting system.

- Python packages live under `packages/` and are managed with `uv`.
- Rust crates live under `crates/` and are managed with Cargo.

## Architecture direction (ADR 0001)

The **deterministic service/run path is Rust** (compile → policy → execution →
ledger → engine → reporter → HTTP API). **Python is a client and data plumbing
only**: it ingests historical data into the Parquet store and, for research,
calls the Rust API and deserializes results for analysis.

The **language boundary is data at rest** — the Parquet market-data store (Python
writes, Rust reads) plus the Rust HTTP API. No domain logic is shared across
languages; the only cross-language overlap is data *shapes* (the JSON-Schema
contracts in `schemas/`, projected to Rust serde and Python Pydantic and guarded
by round-trip fixtures).

| Side | Owns |
| --- | --- |
| **Rust** (`crates/`) | contracts, compile, policy, execution, ledger, engine, Parquet loader, reporter, orchestration, HTTP API |
| **Python** (`packages/`) | data-source adapters + ingestion (write the store); analysis/notebooks (client of the Rust API) |

> The current code still has parts of the run path in Python (compiler, reporter,
> worker, API); these are being moved to Rust per
> [ADR 0001](docs/adr/0001-language-boundary.md) (tracked migration). Until then,
> [docs/system-design.md](docs/system-design.md) describes both the current
> (transitional) layout and the target.

## Local Setup

```bash
uv sync
cargo check --workspace
```

Useful repo checks:

```bash
make check
```

Beginner-friendly domain docs:

- [Crypto trading primer](docs/crypto-trading-primer.md)
- [Market data primer](docs/market-data-primer.md)
- [Chains, ledgers, and venues](docs/chains-ledgers-and-venues.md)
- [Simulation policies](docs/simulation-policies.md)
