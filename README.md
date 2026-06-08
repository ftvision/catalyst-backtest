# Catalyst Backtest

A graph-driven backtesting system for crypto strategies. You build a strategy as
a graph of nodes (swaps, perp orders, yield, signals), pick a period, interval,
and starting portfolio, and the engine replays historical market data to produce
an equity curve, trades, costs, and a summary.

- Rust crates live under `crates/` and are managed with Cargo.
- Python packages live under `packages/` and are managed with `uv`.

## Repo layout

| Path | What |
| --- | --- |
| `crates/` | The deterministic run path in Rust: contracts, graph-compiler, simulation-policies, execution-models, portfolio-ledger, simulation-engine, result-reporter, market-data-loader, and the `simulation-service` HTTP API. |
| `packages/` | Python: the `catalyst-contracts` Pydantic models, market-data ingesters (`market-data`, `market-data-core`, `market-data-dune`, `market-data-bigquery`), and the `catalyst-client` CLI. |
| `apps/web/` | The web workbench (deployed to Cloudflare Pages). |
| `schemas/` | Language-neutral JSON Schemas — the cross-language contract, with round-trip example fixtures. |
| `strategies/` | Bundled example strategy graphs + market scenarios. |
| `scripts/` | Data-ingestion + maintenance scripts (Dune/Hyperliquid fetchers, R2 upload, candle cleaning). |
| `infra/` | Fly.io (API) and Cloudflare (web) deploy config. |
| `docs/` | Design docs and beginner-friendly primers. |

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
| **Python** (`packages/`) | data-source adapters + ingestion (write the store); the CLI client + analysis/notebooks (client of the Rust API) |

> The run path is **entirely Rust** — the migration tracked by
> [ADR 0001](docs/adr/0001-language-boundary.md) (#43) is complete. Python is now
> only data ingestion, the CLI client, and the shared contract models.
> [docs/system-design.md](docs/system-design.md) describes the architecture in
> depth.

## Local setup

```bash
uv sync                  # Python workspace
cargo check --workspace  # Rust workspace
make check               # repo checks (rust + python lint/compile)
make test                # full test suite across both languages
```

## Run a backtest (CLI)

The `catalyst-bt` CLI (package `catalyst-client`) is the command-line client for
the service. A backtest is **graph + config + policy**; the graph is its own JSON
file and a `run.toml` carries the rest. Market data is loaded server-side from
the store for the run's window.

```bash
# Run a bundled example and wait for the result (prints a summary table)
uv run catalyst-bt run packages/client/examples/run.toml --wait

# Inspect before running
uv run catalyst-bt preview run.toml     # validate graph + data requirements
uv run catalyst-bt coverage run.toml    # per-series coverage for the window
uv run catalyst-bt catalog              # what market data the store has
```

The target service is set by `--api-url` or `$CATALYST_API_URL` (defaults to the
deployed API). See [`packages/client/README.md`](packages/client/README.md) for
the full command list.

## Deployed services

| Service | URL |
| --- | --- |
| API (Fly.io) | <https://catalyst-backtest-api.fly.dev> |
| Web workbench (Cloudflare Pages) | <https://catalyst-backtest-web.pages.dev> |

Deploy config is under [`infra/`](infra/).

## Market data

Python ingesters write a Parquet **series store** (candles, funding, gas, yields)
that the Rust loader reads — locally or from object storage (Cloudflare R2). The
concrete sources we fetch (Dune, Hyperliquid, DefiLlama, Binance), the fetch
commands, and the R2 upload step are documented in
[docs/data-sources.md](docs/data-sources.md).

## Docs

Design:

- [System design](docs/system-design.md) — current (transitional) + target layout
- [Data sources](docs/data-sources.md) — what we fetch and how to publish it
- [Market-data storage](docs/market-data-storage.md) and
  [intervals (1h vs 4h)](docs/market-data-intervals.md)
- [ADR 0001 — language boundary](docs/adr/0001-language-boundary.md),
  [ADR 0002 — strategy surface](docs/adr/0002-strategy-surface.md)
- [System logic reference](docs/logic/README.md) — per-component logic (what each
  option does, why, when), backed by tests

Beginner-friendly primers:

- [Crypto trading primer](docs/crypto-trading-primer.md)
- [Market data primer](docs/market-data-primer.md)
- [Chains, ledgers, and venues](docs/chains-ledgers-and-venues.md)
- [Simulation policies](docs/simulation-policies.md)
