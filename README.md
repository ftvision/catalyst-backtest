# Catalyst Backtest

Foundation repo for a graph-driven backtesting system.

The first version keeps the system modular without making the module system itself
too clever:

- Python packages live under `packages/` and are managed with `uv`.
- Rust crates live under `crates/` and are managed with Cargo.
- Python handles orchestration, graph compilation, market data, jobs, and reporting.
- Rust handles deterministic simulation, portfolio accounting, and execution models.
- The first integration boundary is HTTP: Python worker -> Rust simulation service.

See [docs/system-design.md](docs/system-design.md) for the current architecture.

Beginner-friendly domain docs:

- [Crypto trading primer](docs/crypto-trading-primer.md)
- [Market data primer](docs/market-data-primer.md)
- [Chains, ledgers, and venues](docs/chains-ledgers-and-venues.md)
- [Simulation policies](docs/simulation-policies.md)
