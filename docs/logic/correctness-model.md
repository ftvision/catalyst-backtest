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
5. **Fill deferred `next_open` market orders** queued on *earlier* bars —
   `fill_pending_market` (fills at *this* bar's **open**, the earliest price of
   the bar, with taker slippage).
6. **Fill resting limit orders** placed on *earlier* bars — `fill_resting_orders`
   (they fill when the price *touches* their limit intra-bar, after the open).
7. **Run initial actions** (first tick only) — graph actions with no signal gate.
8. **Evaluate signals** — `evaluate_signals`; a firing signal runs its downstream
   actions, which **decide** this tick but, under `next_open`, **queue** (don't
   execute) their market orders for the next bar.
9. **Snapshot equity** and record the portfolio.

A market order (initial or signal-driven) decided at bar *N* under `next_open`
(the `strict_v1` default) is **deferred**: it is queued in step 8 and *filled and
booked* at bar *N+1*'s open in step 5, with the `action_executed` event stamped at
bar *N+1*'s `ts` (`fill_pending_market`). So a position only appears once it has
actually filled, and bar *N*'s accrual / liquidation / snapshot see only positions
really held — no phantom entry-bar P&L (#116, fixed). The deferral mirrors the
resting-limit discipline: deferred markets fill at the **open** (step 5), resting
limits at a later intra-bar **touch** (step 6), so when a take-profit limit and a
deferred entry land on the same bar the entry opens before the limit can reduce it.

```
bar N:  fill markets queued from bar N-1 (at bar N's open) + resting limits
          → ... → evaluate signals on bar N → *queue* market orders for bar N+1
          → snapshot (sees only filled positions)
bar N+1: fill the queued order at bar N+1's open, book it here
```

A market order decided on the **final** bar has no next bar to fill against, so it
lapses unfilled (recorded as `order_expired`) rather than falling back to the
final close — that fallback would be the same-bar look-ahead the deferral exists to
prevent. Same-bar selections like `close`/`mid`/`worse_side_ohlc` still fill in-bar
by design — that's the deliberate "trade-on-close" convention of those profiles,
kept per the #122 decision and flagged by one unconditional run-level warning.

## Correctness invariants

These are the properties the engine aims to guarantee. Where one is only
*partially* held today, it's flagged with the tracking issue.

### 1. No look-ahead (decisions use only past/known data)

A backtest must never act on information unavailable at decision time.

- **Signals** read the current and past bars only; **derived sources**
  (sma/ema/rolling/roc) look strictly backward (see
  [derived-sources](derived-sources.md)).
- **Fill price & timing**: under `next_open` (the `strict_v1` default) an order
  decided on bar *N*'s close is deferred and **both filled and booked** at bar
  *N+1*'s **open** — no same-bar look-ahead on price, and no phantom entry-bar P&L
  from booking on the decision bar (#116, fixed; see
  [fill-price-selection](fill-price-selection.md)).
- **Same-bar selections are a decided, warned convention (#122):**
  `close`/`open`/`mid`/`worse_side_ohlc` fill on the *just-observed* bar, so a
  signal computed from bar *N*'s close fills at bar *N*'s close — the standard
  "trade-on-close" convention (backtesting.py `trade_on_close=True`, Backtrader
  `cheat_on_close`), kept on purpose because deferring a close-fill would fill at
  bar *N+1*'s close (look-ahead the other way). `research_v1` defaults to `close`,
  so every run under a non-`next_open` selection carries **one unconditional
  run-level warning** naming the bias direction; `next_open` is the only
  look-ahead-free selection and the only warning-free one.

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
- ⚠️ **Tracked limitations:** an unpriced non-stable holding (cash or yield)
  is silently dropped from equity and a perp without a mark loses its PnL (#119);
  the venue-scoped carry-forward is unbounded-stale (#119); cumulative
  `total_yield_usd` / `interest_usd` carry asset units for non-stables (#166).

## What is correct today vs. tracked

| Area | Status |
| --- | --- |
| Funding/yield accrual over elapsed time | ✅ fixed (#118) |
| Leverage loss capped at margin; no negative-settlement clawback | ✅ fixed (#117) |
| `amm_price_impact` falls back to `fixed_bps` (never silent zero) | ✅ fixed (#136) |
| `volume_based` slippage (square-root law) | ✅ implemented (#137) |
| Policy contract accepts every model the engine supports | ✅ fixed (#123) |
| `next_open` market orders deferred to fill+book on the fill bar (no phantom entry P&L) | ✅ fixed (#116) |
| Same-bar fills under `close`/`open`/`mid`/`worse_side_ohlc` selection | ✅ decided convention + per-run warning (#122, trade-on-close) |
| Venue-scoped position marking (no cross-venue price borrowing) | ✅ fixed (#119(a)) |
| Staleness bound, unpriced-leg warning, sizing unification, same-tick snapshot | ⚠️ open (#119(b-e)) |
| Non-stable yield positions marked to price; gas converted to asset units | ✅ fixed (#115) |
| `total_yield_usd` / `interest_usd` in asset units for non-stables | ⚠️ open (#166) |
| Liquidation marks the intra-bar wick | ✅ fixed (#120 wick half) |
| Liquidation triggers at full bankruptcy only; no maintenance margin | ⚠️ open (#120) |
| Resting limit orders don't reserve balance | ⚠️ open (#124) |
| Resting limit fills at limit-or-better (maker); AMM impact never reprices them | ✅ fixed (#162) |
| Yield compounds per tick on principal + accrued | ✅ fixed (#114) |
| `yield_accrual` knob wired: `compound_apy` default / `simple_apr` / `none` off-switch | ✅ fixed (#164) |
| Trace/result metadata echoes the EXECUTED policy, per-run overrides included | ✅ fixed (#157) |
| `slippage_bps` validated under every consuming model + after overrides; policy decimals parse loudly (panic, never silently 0) | ✅ fixed (#163) |
| Unimplemented policy values are REJECTED at validation, never silently ignored | ✅ implement-or-reject |
| `same_tick` ordering variants beyond `topological_order` | ⚠️ rejected until implemented (#141) |
| `missing_optional` variants beyond `warn` | ⚠️ rejected until implemented (#142) |
| `missing_required` `skip_tick`/`forward_fill` (use `warn` or `fail`) | ⚠️ rejected until implemented (#159) |
| `venue_fee_table` fee model | ⚠️ rejected until implemented (#143) |
| Partial fills (`partial_fill`/`clamp_to_available`/`allow_*`) | ⚠️ rejected until implemented (#144) |
| Non-`fixed_usd` gas fallback; `fixed_native` gas model | ⚠️ rejected until implemented (#145, #146) |
| `reduce_only_validation = lenient`; `yield.accrual = protocol_index` | ⚠️ rejected until implemented (#158, #164) |
| Rust enums / Python literals / JSON Schema policy parity | ✅ conformance guard in place (#168) |

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
