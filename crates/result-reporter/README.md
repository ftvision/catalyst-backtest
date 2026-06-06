# catalyst-result-reporter (Rust)

Turns a raw `SimulationTrace` into a user-facing `BacktestResult` matching
[`backtest-result.schema.json`](../../schemas/backtest-result.schema.json). Pure
and deterministic — no I/O.

This is the Rust port of the Python `catalyst_result_reporter`; per
[ADR 0001](../../docs/adr/0001-language-boundary.md) the run path lives in Rust
(migration step 1 of #43).

## Usage

```rust
use catalyst_result_reporter::summarize;

let result = summarize(&trace, provider_coverage /* Vec<serde_json::Value> */, None);
```

`summarize` produces: `summary` (start/final value, PnL, return %, max drawdown,
trade/rejected counts), `equity_curve`, `drawdown_curve`, a `trades` log (from
`action_executed` / `action_rejected` / `liquidation` events), a `costs`
breakdown (fees/gas/funding/yield summed from the event log), the final
portfolio, and `metadata` carrying the resolved policy + data-provider coverage.

Detail fields are read tolerantly (JSON string *or* number), so it consumes
traces regardless of how the engine serialized decimals.

## Tests

```bash
cargo test -p catalyst-result-reporter
```

Mirror the Python reporter cases: summary/drawdown math, empty run,
executed/rejected trades + costs, liquidation logging, funding/yield costs, and
policy/coverage passthrough.
