# catalyst-contracts (Rust)

Shared `serde` structs for the Catalyst backtesting system, kept aligned with the
language-neutral JSON Schemas in [`schemas/`](../../schemas) and mirrored by the
Python models in [`packages/contracts`](../../packages/contracts).

## What's here

| Module | Types |
| --- | --- |
| `graph` | `Graph`, `Node`, `Edge`, typed configs (`SwapConfig`, `PerpOrderConfig`, ...) |
| `policy` | `SimulationPolicy` and nested category structs |
| `request` | `BacktestRequest`, `BacktestConfig`, `ExecutionOverrides` |
| `market_data` | `MarketDataBundle` and series/point structs |
| `trace` | `SimulationTrace`, `Snapshot`, `Event`, `Portfolio`, ... |
| `result` | `BacktestResult`, `Summary`, `Trade`, `Costs`, ... |

## Conventions

- **Decimals are strings** (`pub type Decimal = String`) to preserve precision.
- Free-form areas (`Node.config`, event `detail`) are `serde_json::Value`.
- Reserved-word JSON keys are renamed: `Edge.from` stays `"from"`,
  `Event.event_type` ↔ `"type"`, `SimulationPolicy.yield_` ↔ `"yield"`.

## Usage

```rust
use catalyst_contracts::Graph;

let graph: Graph = serde_json::from_str(raw_json)?;
let back = serde_json::to_string(&graph)?;
```

## Tests

```bash
cargo test -p catalyst-contracts
```

`tests/roundtrip.rs` deserializes every `schemas/examples/*` payload, reserializes
it, and asserts the value is stable — the same fixtures the Python package
validates.
