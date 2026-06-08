# System logic reference

Per-component documentation of **what each piece of the engine does, why, and
when to use which option** — grounded in the code and backed by tests that
demonstrate the behavior. One doc per logical component.

Each doc follows the same shape:
- **What it is** — the rule/formula, with the code location.
- **Which market/venue it applies to** — and where it does *not*.
- **Why choose one option over another** — tradeoffs and a decision guide.
- **Tests that show the difference** — named, runnable, cited.

This is an ongoing effort; the checklist below tracks coverage.

## Components

### Execution models
- [x] [Slippage models](slippage-models.md) — `fixed_bps`, `amm_price_impact`, `volume_based`, `none`
- [ ] Fill-price selection — `close` / `open` / `mid` / `next_open` / `worse_side_ohlc`
- [ ] Fees — `fixed_bps` / `venue_fee_table` / `none`
- [ ] Gas — `historical_fee_history` / `fixed_usd` / `fixed_native` / `none` (+ fallback)
- [ ] Partial fills — `none` / `allow_if_configured` / `always_allow`
- [ ] Limit orders — touch logic, time-in-force, resting/expiry

### Accrual
- [ ] Funding accrual — sign, notional, intra-bar summation
- [ ] Yield accrual — simple-APR math, elapsed-time scaling, valuation

### Strategy surface
- [ ] Signals — triggers (`level`/`crossing`/…), repeat/cooldown
- [ ] Derived sources — `sma`/`ema`/`rolling_high`/`rolling_low`/`roc`
- [ ] Sizing — amount basis (`pct_balance`/`pct_position`/`pct_portfolio`)
- [ ] Same-tick ordering

### Risk & data
- [ ] Liquidation — trigger, marking, settlement
- [ ] Missing-data handling — `missing_required` / `missing_optional`
- [ ] Coverage & intra-series gaps

See also: [simulation-policies.md](../simulation-policies.md) (the policy surface),
[system-design.md](../system-design.md) (architecture).
