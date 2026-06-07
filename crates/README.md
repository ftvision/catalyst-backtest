# Rust Crates

Rust crates are managed with Cargo. Per
[ADR 0001](../docs/adr/0001-language-boundary.md), the deterministic run path is
entirely Rust (the migration is complete).

Crates:

- `contracts` — serde structs for the shared JSON Schemas (`schemas/`); the
  cross-language data contract (mirrors Python `catalyst-contracts`).
- `graph-compiler` — compiles a strategy graph into an executable plan and
  derives its data requirements.
- `simulation-policies` — policy profiles (`strict_v1`, `conservative_v1`,
  `research_v1`) and resolution into the concrete execution knobs.
- `execution-models` — fill / price / slippage / gas models the engine applies.
- `portfolio-ledger` — the balance/position ledger.
- `simulation-engine` — the tick loop that runs a compiled graph over market data.
- `result-reporter` — summarizes a trace into the backtest result (equity curve,
  drawdown, trades, costs).
- `market-data-loader` — reads the Parquet series store (local or object storage)
  into a normalized market-data bundle.
- `simulation-service` — the HTTP API + async worker that orchestrates a run
  in-process (compile → policy → load data → engine → reporter).
