# catalyst-client (`catalyst-bt`)

A Python CLI client for the Catalyst backtest service — the command-line
counterpart to the web UI. The deterministic run path is the Rust service
(see [ADR 0001](../../docs/adr/0001-language-boundary.md)); this package is a
thin, typed client over its HTTP API.

It **reuses** rather than rebuilds:

- [`catalyst-contracts`](../contracts) — the schema-aligned Pydantic models for
  the request/response contract (no re-defined types here).
- `httpx` — HTTP. `typer` + `rich` — the CLI and pretty tables. Stdlib
  `tomllib` — run-file parsing.

## The shape of a run

A backtest is **graph + config + policy + (optional) market data**:

- The **graph** is its own JSON file (build it in the UI, or pull one with
  `catalyst-bt strategies <id> --save graph.json`).
- The **config** (period, interval, starting portfolio, execution) and the
  **policy** profile live in a `run.toml` that references the graph.
- **Market data** is normally omitted — the service loads it from its own store
  for the run's window. Pass `--market-data bundle.json` to send one inline.

See [`examples/run.toml`](examples/run.toml):

```toml
graph = "../../../strategies/graphs/g05_hl_perp_open_long.json"
policy = "strict_v1"

[config]
start = "2025-01-01T00:00:00Z"
end = "2025-06-01T00:00:00Z"
interval = "1h"

[config.initial_portfolio.hyperliquid]
USDC = "10000"

[config.execution]
slippage_bps = "10"
gas_model = "none"
```

## Usage

The service URL comes from `--api-url` or `$CATALYST_API_URL`, defaulting to the
deployed Fly URL.

```bash
# Run and wait for the result (prints a summary table)
uv run catalyst-bt run packages/client/examples/run.toml --wait

# Override fields from the file; save the full result JSON
uv run catalyst-bt run run.toml --start 2024-01-01T00:00:00Z --interval 4h --out result.json

# Inspect before running
uv run catalyst-bt preview run.toml      # validate graph + data requirements
uv run catalyst-bt coverage run.toml     # per-series coverage for the window
uv run catalyst-bt catalog               # what market data the store has
uv run catalyst-bt policies              # available policy profiles

# Work with prior runs / a running run
uv run catalyst-bt run run.toml --no-wait        # just enqueue, print the id
uv run catalyst-bt status <id>
uv run catalyst-bt result <id> --json
uv run catalyst-bt events <id> --status rejected
uv run catalyst-bt list --graph-hash <hash>

# Bundled strategies
uv run catalyst-bt strategies
uv run catalyst-bt strategies g05_hl_perp_open_long --save graph.json
```

## Commands

| Command | What it does |
| --- | --- |
| `run <run.toml>` | Submit a backtest; poll to completion (`--wait`, default) and print a summary. `--no-wait` just enqueues. |
| `preview <run.toml>` | Validate the graph; show data requirements + resolved policy (no run). |
| `coverage <run.toml>` | Per-series market-data coverage for the run's window. |
| `catalog` | List the market-data series in the service's store. |
| `policies` | List policy profiles. |
| `strategies [id]` | List bundled strategies, or fetch one's graph (`--save`). |
| `status <id>` / `result <id>` / `events <id>` | Inspect a run by id. |
| `list` | Run history (`--graph-hash` to filter). |

Common `run`/`result` flags: `--start/--end/--interval/--policy/--graph`
(override the file), `--out file.json` (save full result), `--json` (print full
result instead of a summary), `--poll-interval` / `--timeout`.
