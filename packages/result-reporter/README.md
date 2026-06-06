# catalyst-result-reporter

Turns a raw `SimulationTrace` into a user-facing `BacktestResult` matching
[`backtest-result.schema.json`](../../schemas/backtest-result.schema.json). Pure
and deterministic — no I/O, no recomputation of the simulation.

## Usage

```python
from catalyst_result_reporter import summarize

result = summarize(trace, data_coverage=bundle_providers)
```

`summarize` produces:

| Section | From |
| --- | --- |
| `summary` | starting/final value, PnL, return %, max drawdown %, trade/rejected counts |
| `equity_curve` | per-tick snapshot equity |
| `drawdown_curve` | per-tick drawdown vs running peak (+ `max_drawdown_pct`) |
| `trades` | `action_executed` / `action_rejected` / `liquidation` events |
| `costs` | summed fees, gas, funding, yield from the event log |
| `final_portfolio` | carried from the trace |
| `metadata` | **resolved policy** + interval/start/end + **data-provider coverage** + warnings |

The resolved policy and provider metadata are preserved verbatim so every result
stays explainable ("this action failed because strict_v1 rejects insufficient
balances").

## Tests

```bash
uv run pytest packages/result-reporter
```

Cover schema conformance, summary/drawdown math, empty runs, executed/rejected
trades + costs, liquidation events, funding/yield costs, and multi-venue
portfolios.
