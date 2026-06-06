# Schemas

Language-neutral contracts for the system. These JSON Schema files are the
**source of truth**; the Python Pydantic models (`packages/contracts`) and the
Rust Serde structs (`crates/contracts`) are kept aligned with them.

## Schemas

| File | `$id` suffix | Purpose |
| --- | --- | --- |
| `graph.schema.json` | `graph` | Raw Catalyst strategy graph: signal/action nodes + edges. |
| `backtest-request.schema.json` | `backtest-request` | User request: graph + policy selector + run config. |
| `backtest-result.schema.json` | `backtest-result` | Reporter output: summary, curves, trades, costs, assumptions. |
| `market-data-bundle.schema.json` | `market-data-bundle` | Normalized candles/funding/gas/yields fed to the engine. |
| `simulation-policy.schema.json` | `simulation-policy` | Versioned, tunable simulation assumptions (profiles). |
| `simulation-trace.schema.json` | `simulation-trace` | Raw engine output: snapshots + event log + final portfolio. |

All schemas are JSON Schema draft 2020-12 and are individually versioned via a
`schema_version` string (e.g. `catalyst.graph.definition.v1`).

## Conventions

- **Decimals as strings.** Monetary and quantity values are carried as decimal
  strings (e.g. `"100"`, `"0.04"`) to avoid float precision loss across the
  Python ↔ JSON ↔ Rust boundary. The literal `"all"` is allowed for
  full-balance actions in the graph.
- **Timestamps** are RFC 3339 / ISO 8601 UTC strings.
- **Cross-file refs** use absolute `$id` URLs (e.g. a backtest request `$ref`s
  the graph schema). Validators must load all schemas into one registry.

## Examples

`examples/` holds one representative payload per contract. These are the shared
fixtures exercised by **both** the Python contract tests (schema validation +
model parsing) and the Rust contract tests (serde round-trip), which is what
keeps the two languages behaviorally aligned.

| Example | Validates against |
| --- | --- |
| `graph.swap.json`, `graph.perp-signal.json`, `graph.yield.json` | `graph` |
| `backtest-request.json` | `backtest-request` |
| `simulation-policy.strict_v1.json` | `simulation-policy` |
| `market-data-bundle.json` | `market-data-bundle` |
| `simulation-trace.json` | `simulation-trace` |
| `backtest-result.json` | `backtest-result` |
