# catalyst-contracts (Python)

Shared Pydantic v2 models for the Catalyst backtesting system, kept aligned with
the language-neutral JSON Schemas in [`schemas/`](../../schemas) and mirrored by
the Rust structs in [`crates/contracts`](../../crates/contracts).

## What's here

| Module | Models |
| --- | --- |
| `graph` | `Graph`, `Node`, `Edge`, and typed configs (`SwapConfig`, `PerpOrderConfig`, `YieldConfig`, `PriceThresholdConfig`) |
| `policy` | `SimulationPolicy` and its nested category models |
| `request` | `BacktestRequest`, `BacktestConfig`, `ExecutionOverrides` |
| `market_data` | `MarketDataBundle` and its series/point models |
| `trace` | `SimulationTrace`, `Snapshot`, `Event`, `Portfolio`, `PerpPosition`, `YieldPosition` |
| `result` | `BacktestResult`, `Summary`, `Trade`, `Costs`, ... |
| `schemas` | `load_schema`, `validate`, `schemas_dir` helpers |

## Conventions

- **Decimals are strings** (`Decimal = str`) to preserve precision; parse to a
  real decimal at the point of use.
- Envelope models forbid unknown fields (`extra="forbid"`); free-form areas
  (`Node.config`, graph `variables`/`settings`, event `detail`) stay open.
- Reserved-word JSON keys are aliased: `Edge.from_` ↔ `"from"`,
  `SimulationPolicy.yield_` ↔ `"yield"`. Dump with `by_alias=True` to emit the
  wire form.

## Usage

```python
from catalyst_contracts import Graph, validate

raw = {"nodes": [{"id": "n", "kind": "action", "subtype": "swap",
                  "config": {"from_asset": "USDC", "to_asset": "ETH",
                             "amount": "100", "chain": "base"}}]}

validate(raw, "graph")          # JSON Schema validation (needs jsonschema extra)
graph = Graph.model_validate(raw)  # typed model
```

## Tests

```bash
uv run pytest packages/contracts
```

Tests validate every `schemas/examples/*` payload against its schema and parse
it into the matching model.
