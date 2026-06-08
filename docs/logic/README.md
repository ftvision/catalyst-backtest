# System logic reference

Per-component documentation of **what each piece of the engine does, why, and
when to use which option** — grounded in the code and backed by tests that
demonstrate the behavior. One doc per logical component.

Each doc follows the same shape:
- **What it is** — the rule/formula, with the code location.
- **Which market/venue it applies to** — and where it does *not*.
- **Why choose one option over another** — tradeoffs and a decision guide.
- **Tests that show the difference** — named, runnable, cited.

**Start here:** [correctness-model.md](correctness-model.md) — the cross-cutting
correctness guarantees (per-tick execution order, no-look-ahead, money
conservation, elapsed-time accrual, determinism, valuation) and a registry of
what's fixed vs. tracked. The per-component docs below fill in the details.

## Components

### Execution models
- [x] [Slippage models](slippage-models.md) — `fixed_bps`, `amm_price_impact`, `volume_based`, `none`
- [x] [Fill-price selection](fill-price-selection.md) — `close` / `open` / `mid` / `next_open` / `worse_side_ohlc`
- [x] [Fees](fees.md) — `fixed_bps` / `venue_fee_table` / `none`
- [x] [Gas](gas.md) — `historical_fee_history` / `fixed_usd` / `fixed_native` / `none` (+ fallback)
- [x] [Partial fills](partial-fills.md) — partial fills & insufficient-balance handling
- [x] [Limit orders](limit-orders.md) — touch logic, time-in-force, resting/expiry

### Accrual
- [x] [Funding accrual](funding-accrual.md) — sign, notional, intra-bar summation, elapsed-time
- [x] [Yield accrual](yield-accrual.md) — simple-APR math, elapsed-time scaling, valuation

### Strategy surface
- [x] [Signals](signals.md) — triggers (`level`/`crossing`/…), repeat/cooldown
- [x] [Derived sources](derived-sources.md) — `sma`/`ema`/`rolling_high`/`rolling_low`/`roc`
- [x] [Sizing](sizing.md) — amount basis (`pct_balance`/`pct_position`/`pct_portfolio`)
- [x] [Same-tick ordering](same-tick-ordering.md)

### Risk & data
- [x] [Liquidation](liquidation.md) — trigger, marking, settlement
- [x] [Missing-data & coverage](missing-data-and-coverage.md) — `missing_required` / `missing_optional`, intra-series gaps
- [x] [Portfolio valuation](portfolio-valuation.md) — equity / mark-to-market

See also: [correctness-model.md](correctness-model.md) (the cornerstone),
[simulation-policies.md](../simulation-policies.md) (the policy surface),
[production-readiness.md](../production-readiness.md) (the correctness roadmap),
[system-design.md](../system-design.md) (architecture).
