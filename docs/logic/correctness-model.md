# Backtest correctness model

This is the cornerstone of the [logic reference](README.md): the **cross-cutting
correctness guarantees** of the engine, the **per-tick execution order** everything
else hangs off, and an honest **registry of what is and isn't correct yet**. The
per-component docs (slippage, fills, accrual, …) document each piece in detail;
this one ties them together and states the invariants they must uphold.

Everything here is grounded in `crates/simulation-engine/src/engine.rs` (the
`run` loop) and the crates it drives.

## The per-tick execution order

The tick clock is the sorted set of timestamps from the loaded series (candles
preferred; see [missing-data-and-coverage](missing-data-and-coverage.md)). For
each tick `ts`, the engine does, **in this fixed order** (`engine.rs`, the `run`
loop):

1. **Accrue funding** over `(prev_ts, ts]` — `accrue_funding`.
2. **Accrue yield** over the elapsed `ts − prev_ts` — `accrue_yield`.
3. **Check liquidations** — `check_liquidations` (marks at the bar close).
4. **Snapshot tick-start equity** (`tick_equity`) — used to size `pct_portfolio`
   actions this tick.
5. **Fill resting limit orders** placed on *earlier* bars — `fill_resting_orders`
   (they get first claim on the bar before new actions).
6. **Run initial actions** (first tick only) — graph actions with no signal gate.
7. **Evaluate signals** — `evaluate_signals`; a firing signal executes its
   downstream actions *inline, in this same tick*.
8. **Snapshot equity** and record the portfolio.

Why the order matters: accrual happens *before* actions (a position decided this
tick is not yet eligible for this tick's accrual), liquidations are checked before
new risk is added, and resting orders fill before fresh market orders.

## Correctness invariants

These are the properties the engine aims to guarantee. Where one is only
*partially* held today, it's flagged with the tracking issue.

### 1. No look-ahead (decisions use only past/known data)

A backtest must never act on information unavailable at decision time.

- **Signals** read the current and past bars only; **derived sources**
  (sma/ema/rolling/roc) look strictly backward (see
  [derived-sources](derived-sources.md)).
- **Fill price**: under `next_open` (the `strict_v1` default) an order decided on
  bar *N*'s close fills at bar *N+1*'s **open** — no same-bar look-ahead on price
  (see [fill-price-selection](fill-price-selection.md)).
- ⚠️ **Partial gaps remain (tracked):**
  - `close`/`open`/`mid`/`worse_side_ohlc` selections fill on the *just-observed*
    bar, so a signal computed from bar *N*'s close that fills at bar *N*'s close
    is **same-bar look-ahead** — and `research_v1` defaults to `close` (#122).
  - Even under `next_open`, the fill is currently *booked* at the decision bar,
    injecting phantom entry-bar P&L (#116).

### 2. Money conservation (no value created or destroyed out of thin air)

- A leveraged perp's loss is **capped at the posted margin** — an underwater
  close settles at zero rather than crediting negative (clawing back unposted
  collateral). A swap **sell is rejected** when fee+gas exceed proceeds, so no
  negative balance is minted (#117, merged; see [liquidation](liquidation.md)).
- Balance debits are guarded; under non-`allow_negative` policy a balance can't
  go negative through the normal paths.
- ⚠️ **Tracked:** resting limit orders don't yet *reserve* the balance they'd
  spend, so equity can transiently over-count an open order's cash (#124).

### 3. Accrual scales with real elapsed time

Funding and yield accrue over the **actual seconds since the previous tick**
(`ts − prev_ts`), not a static interval — correct across data gaps and when the
configured interval is coarser than the data (#118, merged). Funding additionally
**sums every funding point within the bar**, so a coarse tick over fine funding
accrues all of it (see [funding-accrual](funding-accrual.md),
[yield-accrual](yield-accrual.md)).

### 4. Determinism

The run path is deterministic (Rust, no wall-clock/RNG in the hot path; see
[ADR 0001](../adr/0001-language-boundary.md)). Same inputs → byte-identical trace.
Within a tick, multiple signals/actions resolve in a **defined order** set by the
policy (see [same-tick-ordering](same-tick-ordering.md)). The one `f64` round-trip
in the engine — the `√` in `volume_based` slippage — is IEEE-754 deterministic.

### 5. Valuation reflects markable state

Equity = stable balances (1:1) + non-stable balances (marked to close) + perp
margin and unrealized PnL + yield principal and accrued (see
[portfolio-valuation](portfolio-valuation.md)).
- ⚠️ **Tracked limitations:** non-stable **yield** positions are valued 1:1 as USD
  (wrong for a non-stable deposited asset, #115); an unpriced non-stable holding
  is silently dropped from equity and a perp without a mark loses its PnL (#119);
  the price fallback is venue-blind and unbounded-stale (#119).

## What is correct today vs. tracked

| Area | Status |
| --- | --- |
| Funding/yield accrual over elapsed time | ✅ fixed (#118) |
| Leverage loss capped at margin; no negative-settlement clawback | ✅ fixed (#117) |
| `amm_price_impact` falls back to `fixed_bps` (never silent zero) | ✅ fixed (#136) |
| `volume_based` slippage (square-root law) | ✅ implemented (#137) |
| Policy contract accepts every model the engine supports | ✅ fixed (#123) |
| `next_open` fills booked at the decision bar (phantom entry P&L) | ⚠️ open (#116) |
| Same-bar look-ahead under `close`/`open`/`mid` selection | ⚠️ open (#122) |
| Inconsistent/stale/venue-blind price lookups; equity drops unpriced holdings | ⚠️ open (#119) |
| Non-stable yield position valuation (1:1 USD, gas units) | ⚠️ open (#115) |
| Liquidation marks at close only; no maintenance margin | ⚠️ open (#120) |
| Resting limit orders don't reserve balance | ⚠️ open (#124) |
| Yield is simple-interest, not compounding | ⚠️ fidelity (#121) |

The broader roadmap to production-grade correctness is in
[production-readiness.md](../production-readiness.md) (Tier 0 = "the numbers are
right").

## How correctness is enforced

- **Tests as executable spec.** Each behavior has named tests under
  `crates/*/tests/` (cited in every component doc). Notable correctness suites:
  `no_look_ahead.rs`, `funding_interval.rs`, `accrual_gaps.rs`, `limit_orders.rs`,
  `coverage_gaps.rs`, plus the execution-model unit tests in
  `crates/execution-models/tests/execution.rs`.
- **Schema round-trips.** The JSON-Schema contracts are validated cross-language
  (Rust serde ↔ Python Pydantic) so the data shapes can't drift.
- **Fix discipline.** Every correctness fix lands test-first: a realistic
  failing case that reproduces the bug, then the fix (e.g. the 10× perp closed
  into a 15% crash in #117, the gapped-tick deposit in #118).
