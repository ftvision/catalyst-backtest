# catalyst-simulation-engine

The deterministic heart of the backtester: it runs a graph over normalized
market data and emits a `SimulationTrace`. It **never fetches raw data** — it
reads only the `MarketDataBundle` handed to it.

## Input → output

```rust
use catalyst_simulation_engine::{run, SimulationInput};

let trace = run(&SimulationInput { graph, config, policy, market_data })?;
```

`run` resolves the policy, indexes the bundle, derives the executable graph, then
loops over the candle timestamps in `[start, end]`.

## The tick loop

At each tick, in order:

1. **Funding** — accrue historical funding on open perps (signed; long pays when rate > 0).
2. **Yield** — accrue `principal × apr × (interval / year)` onto yield positions.
3. **Liquidation** — close any perp whose unrealized loss has eaten its margin.
4. **Initial actions** (first tick only) — run actions with no incoming edge, following action→action chains.
5. **Signals** — for each `price_threshold` signal, evaluate the condition and fire per the policy's `signal_trigger`:
   - `crossing` (default): fires only on a false→true transition (so a ladder re-fires when price re-crosses, not every tick it stays below).
   - `level`: fires every tick the condition holds. `once_per_backtest`: fires at most once.
   Firing runs each target action and follows its chains.
6. **Snapshot** — record mark-to-market equity and the full portfolio.

Every action attempt produces an `action_executed` or `action_rejected` event; a
rejection leaves the ledger unchanged. The resolved policy is embedded in the
trace so results are explainable.

## Execution semantics

The engine derives triggers from the raw graph itself (mirroring the Python graph
compiler) and dispatches each action to the execution models:

| subtype | model |
| --- | --- |
| `swap` | EVM / Hyperliquid spot |
| `perp_order` | Hyperliquid perp open/add or reduce-only close |
| `yield_deposit` / `yield_withdraw` | Aave-style |

## Tests

```bash
cargo test -p catalyst-simulation-engine
```

Golden-style tests over synthetic market data cover threshold crossing (fires
once while held), repeated signals (re-fire on re-cross), action chaining,
rejected actions, perp round trips, and policy metadata in the trace.
