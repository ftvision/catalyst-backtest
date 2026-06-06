# System Design

## Goal

Build a backtesting system that accepts a runnable Catalyst graph, a time range,
and execution assumptions, then returns strategy performance, event traces, and
the final portfolio.

The design should support:

- EVM swaps
- Hyperliquid spot swaps
- Hyperliquid perp open/close orders
- EVM yield deposit/withdraw actions
- price-threshold signals
- deterministic replay over historical market data

Out of scope for the first version:

- options
- prediction markets
- live trading
- fully realistic DEX routing
- order-book level execution
- runtime language switching

## Language boundary (ADR 0001 — target architecture)

The deterministic **service/run path is Rust**; **Python is a client + data
plumbing only**. See [adr/0001-language-boundary.md](adr/0001-language-boundary.md).

```mermaid
flowchart LR
  subgraph PY["Python (edges)"]
    Ingest["data-source adapters + ingestion"]
    Research["notebooks / analysis (API client)"]
  end
  subgraph STORE["Boundary = data at rest"]
    Parquet["Parquet market-data store"]
  end
  subgraph RS["Rust (service / run path)"]
    API["HTTP API (axum)"]
    Core["compile → policy → execution → ledger → engine → reporter"]
    Loader["Parquet loader (object_store)"]
  end

  Ingest -->|writes| Parquet
  Parquet -->|read| Loader --> Core
  API --> Core
  Research -->|HTTP| API
```

- **Rust owns**: contracts (serde), compile, policy, execution, ledger, engine,
  Parquet loader, reporter, orchestration, HTTP API.
- **Python owns**: ingestion (writes the store) and analysis (calls the API).
- **No domain logic crosses the boundary.** The only cross-language overlap is
  data *shapes* (the JSON-Schema contracts below), single-sourced and
  fixture-guarded.

This resolves the duplication in #28 and the JSON-bundle boundary in #29.

> **Current vs target.** The sections below describe the system as a Python
> orchestrator (`backtest-api`/`backtest-worker`) calling a Rust simulation
> service. That is the **current, transitional** layout; the run path is being
> moved to Rust per ADR 0001 (migration tracked in #43). Treat the Python
> orchestration/compiler/reporter boxes as transitional.

## Repository Shape

```text
catalyst-backtest/
  Cargo.toml
  pyproject.toml

  schemas/
    graph.schema.json
    backtest-request.schema.json
    backtest-result.schema.json

  crates/
    contracts/
    simulation-policies/
    portfolio-ledger/
    execution-models/
    simulation-engine/
    simulation-service/

  packages/
    contracts/
    graph-compiler/
    market-data/
    backtest-worker/
    backtest-api/
    result-reporter/

  apps/
    web/

  tests/
    fixtures/
    golden/
    conformance/

  infra/
```

Rust packages are crates. Python packages are packages. No `-rs` or `-py`
suffixes are needed because the folder tells us the implementation language.

## System Box Graph

```mermaid
flowchart LR
  Client["Client<br/>Catalyst app, CLI, or web UI"] --> API["packages/backtest-api<br/>Python FastAPI"]

  API --> Compiler["packages/graph-compiler<br/>Python"]
  Compiler --> Planner["packages/market-data<br/>Data planner + fetcher"]
  Planner --> Cache["Market data cache<br/>Parquet / DuckDB"]

  API --> Worker["packages/backtest-worker<br/>Python"]
  Worker --> Cache
  Worker -->|HTTP POST /simulate| SimSvc["crates/simulation-service<br/>Rust Axum"]

  SimSvc --> Engine["crates/simulation-engine"]
  Engine --> Policy["crates/simulation-policies"]
  Engine --> Ledger["crates/portfolio-ledger"]
  Engine --> Exec["crates/execution-models"]

  SimSvc --> Worker
  Worker --> Reporter["packages/result-reporter<br/>Python"]
  Reporter --> Store["Result store<br/>JSON / Parquet / Postgres later"]
  Store --> API
  API --> Client
```

## Package Responsibilities

### `schemas/`

Language-neutral contracts.

Start with JSON Schema because it is easy to inspect, easy to fixture, and works
well across Python, Rust, TypeScript, and HTTP.

Core schemas:

- Catalyst graph
- backtest request
- backtest result
- normalized market data bundle
- simulation trace
- portfolio snapshot
- trade event
- error/warning object

### `packages/contracts`

Python models generated from, or manually aligned to, `schemas/`.

Likely tools:

- Pydantic
- datamodel-code-generator, if generation becomes worth it

### `crates/contracts`

Rust structs aligned to `schemas/`.

Likely tools:

- `serde`
- `serde_json`
- `schemars`, if Rust should emit schemas later

### `packages/graph-compiler`

Validates and normalizes Catalyst graphs.

Responsibilities:

- parse graph JSON
- reject disabled or malformed nodes
- validate edge references
- identify initial actions
- identify signal-driven actions
- normalize action configs into typed internal operations
- produce data requirements for the market-data package

Open semantic decisions:

- whether signal nodes fire on level or crossing
- how repeated actions are represented
- how action-to-action edges should delay or sequence actions
- how to handle multiple incoming edges

### `packages/market-data`

Fetches and caches data needed to run the graph.

Responsibilities:

- inspect compiled graph data requirements
- fetch Hyperliquid candles/funding/metadata
- fetch EVM token prices
- fetch EVM gas data
- fetch Aave/Base yield rates
- normalize data into a simulation-friendly bundle
- cache fetched data locally

Initial data sources:

- Hyperliquid official API for spot/perp candles and funding
- DefiLlama for fallback token prices and yield data
- Aave subgraphs for reserve/yield rates
- Base/EVM RPC for gas/block data
- DEX subgraphs later for pool-level swap execution

### `packages/backtest-worker`

Coordinates a backtest run.

Responsibilities:

- receive validated request from API
- call graph compiler
- call market-data planner/fetcher
- call Rust simulation service over HTTP
- persist raw simulation trace
- call result reporter
- persist final result artifacts

### `crates/simulation-service`

HTTP wrapper around the Rust simulation engine.

First endpoint:

```http
POST /simulate
```

Input:

- compiled graph
- backtest config
- normalized market data bundle
- initial portfolio

Output:

- simulation trace
- final portfolio
- event list
- error/warning list

### `crates/simulation-engine`

Pure deterministic simulation.

Responsibilities:

- run the event loop
- evaluate signal state at each tick
- schedule executable actions
- call execution models
- update portfolio ledger
- accrue funding and yield
- mark positions to market
- produce snapshots and events

The engine should not fetch raw market data.

### `crates/simulation-policies`

Centralized rules for ambiguous or tunable simulation behavior.

Responsibilities:

- insufficient balance behavior
- partial fill behavior
- execution price selection
- slippage model selection
- fee model selection
- gas model selection
- signal trigger behavior
- same-tick ordering
- missing data behavior
- perp risk policy
- yield accrual policy

Policies should be explicit, versioned, serializable, validated, and included in
every backtest result.

See [simulation-policies.md](simulation-policies.md).

### `crates/portfolio-ledger`

Deterministic accounting.

Responsibilities:

- cash balances by venue/chain
- token balances by venue/chain
- spot inventory
- perp positions
- collateral and margin
- realized and unrealized PnL
- fee, gas, funding, and yield entries
- final portfolio valuation

### `crates/execution-models`

Venue/action simulation.

Initial models:

- Hyperliquid spot market swap
- Hyperliquid perp market open/close
- EVM swap with price + fee + slippage + gas approximation
- Aave-style yield deposit/withdraw with rate accrual

### `packages/result-reporter`

Turns raw trace into user-facing results.

Responsibilities:

- equity curve
- drawdown
- final portfolio
- trade log
- position history
- costs breakdown
- assumptions summary
- warnings and data coverage notes

## Request Flow

```mermaid
sequenceDiagram
  participant C as Client
  participant A as Backtest API
  participant G as Graph Compiler
  participant D as Market Data
  participant W as Worker
  participant S as Rust Simulation Service
  participant R as Result Reporter

  C->>A: POST /backtests
  A->>G: validate + compile graph
  G-->>A: compiled graph + data requirements
  A->>W: enqueue run
  W->>D: fetch normalized market data
  D-->>W: market data bundle
  W->>S: POST /simulate
  S-->>W: simulation trace
  W->>R: summarize trace
  R-->>W: backtest result
  W-->>A: persist result
  C->>A: GET /backtests/{id}
```

## First API Shape

```http
POST /backtests
GET /backtests/{id}
GET /backtests/{id}/result
GET /backtests/{id}/events
```

`POST /backtests` accepts:

```json
{
  "graph": {},
  "policy": {
    "profile": "strict_v1"
  },
  "config": {
    "start": "2024-01-01T00:00:00Z",
    "end": "2024-06-01T00:00:00Z",
    "interval": "1h",
    "initial_portfolio": {
      "base": { "USDC": "1000" },
      "hyperliquid": { "USDC": "1000" }
    },
    "execution": {
      "signal_trigger": "crossing",
      "slippage_bps": "10",
      "gas_model": "historical",
      "action_cooldown": "1h"
    }
  }
}
```

## Simulation Defaults

Initial defaults should be explicit and visible in every result:

- bar-based simulation
- `1h` interval unless configured otherwise
- signal fires on crossing, not continuously
- initial actions execute at start time
- action-to-action edges execute immediately after prior success
- fixed slippage until better venue models exist
- historical gas where available, fixed fallback otherwise
- insufficient balance causes action rejection, not negative balances
- perp liquidation is checked every tick

These defaults should be implemented through centralized simulation policies, not
scattered through execution code. See [simulation-policies.md](simulation-policies.md).

## Data Strategy

Use a normalized data bundle as the input to Rust.

Benefits:

- Rust stays deterministic
- fixtures are easy to test
- data source changes do not force simulation changes
- same simulation can run from cached data, fixtures, or live fetches

Possible storage:

- local Parquet for candles/rates/events
- DuckDB for local querying
- Postgres later for job metadata
- object storage later for large artifacts

## Testing Strategy

### Fixture Tests

Small deterministic market data fixtures:

- ETH flat
- ETH crosses below threshold
- ETH crosses above threshold
- ETH gaps through multiple thresholds
- funding positive/negative
- gas spike
- yield rate changes

### Golden Graphs

Use the sample graphs from the problem statement.

Each golden test should define:

- graph
- config
- initial portfolio
- market data fixture
- expected trades
- expected final balances
- expected warnings/errors

### Conformance Tests

If a package moves language in the future, it must pass the same fixtures. We do
not need runtime language switching, but we do need stable behavior.

## UI/UX Notes

The first wireframe was useful because it exposed what probably does not work:

- the app may not want a large freeform graph canvas as the dominant surface
- the core workflow may be closer to "configure, run, inspect assumptions,
  compare outcomes" than "draw a graph"
- Catalyst may already own graph creation, so this app might be primarily a
  backtest review and debugging surface

Open UI questions:

- Is the backtest UI embedded inside Catalyst, or is it a separate workbench?
- Does the user edit graphs here, or only inspect graphs produced elsewhere?
- Should the primary screen be run setup, result analysis, or graph debugging?
- How much should the UI expose low-level assumptions such as slippage, gas,
  funding, fill model, and trigger behavior?
- Who is the first user: strategy creator, protocol engineer, investor/researcher,
  or internal QA?

Potential product surfaces:

1. **Run setup:** graph summary, time range, portfolio, assumptions, data coverage.
2. **Result analysis:** equity, drawdown, trades, final portfolio, costs, assumptions.
3. **Graph debugging:** why each signal fired, why actions executed/rejected.
4. **Comparison:** compare configs, intervals, or execution assumptions.
5. **Data audit:** show source coverage, missing periods, stale data, fallbacks.

The next UI discussion should start from the user's workflow, not from visual
layout. The most important question is: what decision is the user trying to make
after a backtest finishes?
