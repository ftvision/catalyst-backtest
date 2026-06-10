# Yield accrual & valuation

**Yield** models an Aave-style deposit that earns interest over time. A
`yield_deposit` action moves principal off the chain balance into a yield
position; the engine then accrues interest **every tick** based on the elapsed
wall-clock time and the position's current APR; a `yield_withdraw` redeems
principal + accrued back to the chain balance. Correctness hinges on three
things: accrual scaling with *actual* elapsed seconds, money conservation across
deposit/accrue/withdraw, and reporting in honest units: interest accrues in
**asset units** (interest on an ETH deposit is ETH) while the cumulative USD
counters carry that interest converted at the accrual tick's mark price
(#166, fixed).

The deposit/withdraw execution lives in
`crates/execution-models/src/yields.rs`; the per-tick accrual and valuation live
in `crates/simulation-engine/src/engine.rs`; the position math lives in
`crates/portfolio-ledger/src/lib.rs` and `.../position.rs`.

## What it is

### The accrual rule (per-tick compounding)

Every tick, for every open yield position, the engine credits

```
interest = (principal + accrued) · apr · fraction
fraction = elapsed_secs / YEAR_SECONDS        (YEAR_SECONDS = 31_536_000)
```

`crates/simulation-engine/src/engine.rs:1192` (`accrue_yield`):
- `fraction` is computed once per tick from `elapsed_secs`
  (`engine.rs:1201`); `YEAR_SECONDS = 31_536_000` (`engine.rs:27`).
- `elapsed_secs` is `ts - prev_ts` (the actual gap since the previous tick), or
  the configured `interval_secs` on the very first tick when there is no prior
  tick (`engine.rs:264`).
- `apr` is looked up per position via `index.apr_at(key, ts)`
  (`engine.rs:1204`); if there's no APR series for the position at all, that
  position is skipped this tick (`else { continue }`).
- `interest = (y.principal + y.accrued) · apr · fraction` (`engine.rs:1209`).
  The base is the position's full value — previously-accrued interest earns
  interest, i.e. it **compounds** (#114, fixed). Because `withdraw_yield`
  draws accrued-first-then-principal, the base equals `YieldPosition::value()`
  regardless of how withdrawals were split.
- Zero interest is skipped (no event); otherwise the interest is converted to
  USD — `price = 1` for stables, else the venue-scoped carry-forward mark
  `mark_price(index, chain, asset, ts)`, the same convention as
  `compute_equity` (#166) — then it calls `ledger.accrue_yield(...)` and emits
  a `yield_accrued` event carrying `apr`, `interest` (asset units), `price`,
  and the converted `interest_usd`.
- Defensive gap behavior (#166): a mark provably exists after a non-stable
  deposit (deposits with no price are rejected, #115), but if one is ever
  absent at an accrual tick the asset-unit accrual still happens — economics
  must not depend on price availability — while `interest_usd` is 0 and a
  run-level warning is pushed ("total_yield_usd under-counts").

`ledger.accrue_yield` (`crates/portfolio-ledger/src/lib.rs`) adds the
asset-unit interest to `position.accrued` and the caller-converted
`interest_usd` to the cumulative `yield_usd` counter (#166 — the two differ
for non-stables, and the caller owns the conversion); it errors (`NoSuchYield`)
if the position doesn't exist, which the engine ignores with `let _`, since it
only iterates over positions it just snapshotted.

### Deposit / withdraw (the balance moves)

`execute_yield_deposit` (`yields.rs:22`):
1. Compute gas in USD via `gas_usd(chain, ...)` (`yields.rs:30`).
2. Resolve amount: a fixed string is parsed; `"all"` reserves gas first —
   `(balance − gas).max(0)` (`yields.rs:32`).
3. Reject if the amount is zero (`yields.rs:38`).
4. `ledger.deposit_yield(...)` debits principal from the chain balance and adds
   it to the position's `principal`, creating the position with `accrued = 0` if
   new (`lib.rs:207`).
5. `ledger.debit(chain, asset, gas)` charges gas, then `record_gas(gas)`
   (`yields.rs:45–48`).

`execute_yield_withdraw` (`yields.rs:64`):
1. Resolve amount: `"all"` ⇒ `ledger.yield_value(...)` (principal + accrued);
   else parse the fixed amount (`yields.rs:74`).
2. Reject if zero (`yields.rs:79`).
3. `ledger.withdraw_yield(...)` validates and moves value back to the chain
   balance, then gas is debited and recorded (`yields.rs:83–89`).

`ledger.withdraw_yield` (`lib.rs:255`):
- Rejects with `InsufficientYield` if `amount > position.value()`
  (principal + accrued) (`lib.rs:270–278`).
- **Draws accrued interest first, then principal** (`lib.rs:280–282`):
  `from_accrued = amount.min(accrued)`, then `principal -= amount − from_accrued`.
- Removes the position entirely once its value hits zero (`lib.rs:283`).
- Credits the withdrawn amount back to the chain balance (`lib.rs:286`).

`YieldPosition::value() = principal + accrued`
(`crates/portfolio-ledger/src/position.rs:89`).

### Valuation in equity

`compute_equity` (`engine.rs:1279`) values every open yield position like a
spot balance (`engine.rs:1300-1309`): a stable asset counts `y.value()`
(= principal + accrued) 1:1; a non-stable asset counts
`y.value() × mark_price`. An unpriced non-stable position is silently skipped
(#119) — see the correctness notes below.

### Policy knobs (and which are honored)

| Policy field | Variants | Honored? |
| --- | --- | --- |
| `yield_accrual` | `CompoundApy` (default), `SimpleApr`, `None`, `ProtocolIndex` | **Yes — wired** (#164). `compound_apy` compounds on principal + accrued; `simple_apr` accrues on principal alone; `none` turns accrual off (symmetric with `perps.funding = none`); `protocol_index` is rejected at validation until implemented. |

`accrue_yield` dispatches on the knob (`crates/simulation-engine/src/engine.rs`,
the `policy.yield_accrual` match): `none` returns before any accrual, and the
interest base is `principal` (simple) vs `principal + accrued` (compound). All
three profiles default to `compound_apy` — the resolved policy now names the
behavior that actually runs. The three variants are differentiated by
`yield_accrual_variants_differentiate`
(`crates/simulation-engine/tests/honest_policy_surface.rs`).

## Which market / when to use

Yield positions model lending-protocol deposits (Aave-style) on a chain — a way
to park stable capital and earn APR between trades, or to backtest a
yield-bearing leg of a strategy. The APR comes from a per-(protocol, asset,
chain, pool) yield series in the market-data bundle (see the `accrual_gaps.rs`
test bundle for the shape: a `yields` entry with a `points` array of
`{ts, apr}` points).

There is currently only one accrual behavior (per-tick compounding), so there's
no "choose one over another" decision to make at the policy level. Stable and
non-stable deposit assets are both valued correctly in equity (#115, fixed)
and in the cumulative reporting counters (#166, fixed).

## Correctness notes / edge cases

- **Accrual over actual elapsed time, not a fixed interval (#118).** The tick
  clock is data-driven and can be gapped (e.g. a missing candle/yield point). A
  position held across a gap accrues the *whole* elapsed interval because
  `fraction` uses `ts − prev_ts`, not the nominal `interval_secs`
  (`engine.rs:241`, `engine.rs:1028`). The pre-fix code charged only one
  interval's worth at the post-gap tick, silently dropping the gap time.
- **APR is forward-filled.** `apr_at` returns the exact point at `ts`, else the
  last known APR at or before `ts` (`crates/simulation-engine/src/market.rs:182`,
  `m.range(..=ts).next_back()`). So a sparse APR series holds its last value
  forward — and never reads a *future* APR (no look-ahead). If there is no point
  at or before `ts`, the position is skipped that tick.
- **No look-ahead in accrual.** Accrual at tick `ts` uses only `principal`
  (already on the books) and `apr_at(ts)` (past/current data). It runs at the
  *start* of the tick, before any of this tick's actions
  (`engine.rs:245`, ahead of `run_action_chain` / `evaluate_signals` later in the
  loop), so newly-deposited principal does not accrue for the tick on which it
  was deposited — interest first appears on the *next* tick.
- **Compounding follows the tick grid (#114 — FIXED).** Interest is
  `(principal + accrued) · apr · fraction` (`engine.rs:1209`), i.e. discrete
  compounding whose frequency is the **data-driven tick grid**: dense hourly
  data compounds hourly; a long gap compounds once over the whole elapsed
  window (simple within the gap). Two stated assumptions: (a) the input series
  field is an **APR** (per-tick compounding of an APR approaches e^APR
  effective on dense grids); if a provider actually delivers a published *APY*,
  this slightly double-compounds; (b) results are mildly grid-dependent —
  the same position over the same wall-clock accrues marginally more on a
  finer grid. Both tracked under #164.
- **The yield-policy gate exists (#121(b)/#164 — FIXED).** `accrue_yield`
  consults `policy.yield_accrual`: `none` disables accrual entirely (the
  off-switch, symmetric with `Funding::None`), `simple_apr` selects
  principal-only interest, `compound_apy` (default) compounds. Remaining #164
  scope: a `protocol_index` model (rejected until implemented) and the
  APR-vs-APY input assumption below.
- **Non-stable yield marked to price (#115 — FIXED).** `compute_equity` values
  a non-stable yield position as `y.value() × mark_price` like the spot branch;
  stables stay 1:1 (`engine.rs:1300-1309`; `is_stable`,
  `crates/execution-models/src/pricing.rs:15`). An *unpriced* non-stable yield
  position is silently skipped from equity — the same gap as the cash branch
  (#119). The accrual itself stays unit-agnostic (`apr` applies to asset-unit
  principal), which is correct: interest is earned in the deposited asset.
- **Yield counters report USD, not asset units (#166 — FIXED).** The
  cumulative `yield_usd` counter and the `yield_accrued` event's
  `interest_usd` carry the interest converted at the accrual tick's
  carry-forward mark (venue = chain), the same convention as `compute_equity`;
  stables convert at 1 without a candle lookup. The event also reports
  `interest` (asset units) and `price` for audit, and the reporter's
  `total_yield_usd` sums the converted field. If a non-stable mark is ever
  absent at an accrual tick (it shouldn't be — unpriced deposits are
  rejected), the asset-unit accrual still happens, `interest_usd` is 0, and a
  run-level warning flags the under-count.
- **Gas converted to asset units at the tick price (#115 — FIXED).** `gas_usd`
  returns USD (`crates/execution-models/src/pricing.rs:102`); deposit and
  withdraw now debit `gas / price` asset units (`yields.rs:44-52`, and the
  `"all"` path reserves the converted amount). For a stable asset the price is
  1 (unchanged). A non-stable deposit/withdraw with **no price this tick is
  rejected** with the ledger untouched, rather than mixing units.
  `record_gas(gas)` stays in USD. (`hyperliquid` gas is zero, `pricing.rs:103`;
  `GasModel::HistoricalFeeHistory` falls back to the policy's fixed amount when
  no gas series exists, `pricing.rs:109–111`.)
- **Money conservation across the lifecycle.** Deposit moves exactly `amount`
  from chain balance into `principal` (`lib.rs:215`, `lib.rs:220`); accrual only
  ever *adds* to `accrued`/`yield_usd` (`lib.rs:248–249`); withdraw moves exactly
  `amount` back to the chain balance, drawing accrued-then-principal so the
  position can never go negative (`lib.rs:280–286`); over-withdrawal is rejected
  (`lib.rs:270`). Gas is a separate, additional debit recorded in the cumulative
  gas counter. The withdraw drains `accrued` before `principal`, so a partial
  withdraw of ≤ accrued leaves principal intact.
- **Atomicity of deposit/withdraw.** Each has two fallible balance moves (the
  principal move and the gas debit). Per the module doc (`yields.rs:8–10`), the
  engine runs every action on a trial copy of the ledger and commits only on
  success, so a partway failure (e.g. principal moves but gas can't be covered)
  is discarded wholesale — no manual rollback. Demonstrated by
  `yield_deposit_failing_on_gas_leaves_ledger_untouched` in
  `crates/simulation-engine/tests/engine.rs:238`.
- **Determinism.** All arithmetic is `rust_decimal::Decimal` (no floats in the
  run path); `fraction = elapsed_secs / YEAR_SECONDS` is exact-rational; APR
  lookup is a deterministic BTreeMap range query. The `accrual_gaps.rs` test
  asserts the accrued total to within `1e-9` of the closed-form value (after an
  f64 round-trip in the test harness only — the run path itself stays Decimal).

## Tests

`crates/simulation-engine/tests/accrual_gaps.rs`:
- `yield_accrues_full_elapsed_time_across_a_tick_gap` — deposits 10,000 USDC at
  5% APR over a series with a missing 2h candle (points at 0h/1h/3h). Accrual
  fires 1h at tick 1 and 2h across the gap at tick 3 (the gap slice compounding
  on the tick-1 interest), proving #118 — the pre-fix static-interval value is
  explicitly checked against.

`crates/simulation-engine/tests/issue_114_yield_simple_not_compounding.rs`:
- exact-`Decimal` equality against an independently computed per-tick
  compounding reference loop, a negative control against the old
  simple-interest total, and a first-tick boundary case (compounding equals
  simple interest while `accrued = 0`).

`crates/portfolio-ledger/tests/ledger.rs`:
- `deposit_yield_debits_and_creates_position` — principal moves off the balance,
  position created with `accrued = 0`.
- `accrue_then_withdraw_all_returns_principal_plus_interest` — accrue 1.25 on
  250, `yield_value` = 251.25, withdraw-all returns 251.25 and removes the
  position.
- `partial_withdraw_draws_accrued_first` — accrue 5, withdraw 3: `accrued` drops
  to 2 while `principal` stays 250 (accrued-first ordering).
- `overdraw_yield_is_rejected` — withdrawing more than `value()` returns
  `InsufficientYield`.

`crates/execution-models/tests/execution.rs`:
- `yield_deposit_moves_principal_and_charges_gas` — 300 → 49.98 (250 principal +
  0.02 gas), confirming the separate gas debit.
- `yield_deposit_insufficient_is_rejected` — deposit larger than balance is
  rejected and the balance is untouched (trial-ledger atomicity).
- `yield_withdraw_partial_returns_funds` — withdraw 100 of 250; balance lands at
  149.96 (49.98 + 100 − 0.02 gas) and remaining principal updates, gas charged
  again.
- `yield_withdraw_all_empties_position` — `"all"` redeems and removes the
  position.

`crates/simulation-engine/tests/engine.rs`:
- `yield_deposit_failing_on_gas_leaves_ledger_untouched` — end-to-end atomicity:
  a deposit whose gas can't be covered leaves the real ledger unchanged.

`crates/simulation-engine/tests/issue_115_nonstable_yield_equity.rs` and
`issue_115_nonstable_yield_gas_and_value.rs` pin the non-stable paths: equity
marked to price (2000 → 1800 on a price move, never $1/unit), gas debited as
`gas/price` asset units, `value_usd = amount × price` on fills, the
no-price-rejects-with-untouched-ledger case, and the #114×#115 cross-product
(a non-stable position accruing compounded interest, equity =
`(principal + accrued) × price`, with the `yield_accrued` event's
`interest_usd` = interest × mark and a stable control where
`interest_usd == interest`, #166). The reporter test
`funding_and_yield_costs_summed` (`crates/result-reporter/tests/reporter.rs`)
pins that `total_yield_usd` sums the converted `interest_usd` field, not the
asset-unit `interest`. (No test exercises `CompoundApy` / `ProtocolIndex`,
since they have no behavior — #164.)

## Related issues

- [#114](https://github.com/ftvision/catalyst-backtest/issues/114) — yield compounding — FIXED
- [#115](https://github.com/ftvision/catalyst-backtest/issues/115) — non-stable yield valuation — FIXED
- [#118](https://github.com/ftvision/catalyst-backtest/issues/118) — elapsed-time accrual — FIXED
- [#121](https://github.com/ftvision/catalyst-backtest/issues/121) — fidelity backlog (pct_position, cooldown/TIF)
- [#164](https://github.com/ftvision/catalyst-backtest/issues/164) — `yield_accrual` knob unwired; APR-vs-APY input assumption; no off-switch
- [#166](https://github.com/ftvision/catalyst-backtest/issues/166) — `total_yield_usd`/`interest_usd` converted to USD at the accrual tick's mark — FIXED
