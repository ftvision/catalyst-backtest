# Yield accrual & valuation

**Yield** models an Aave-style deposit that earns interest over time. A
`yield_deposit` action moves principal off the chain balance into a yield
position; the engine then accrues interest **every tick** based on the elapsed
wall-clock time and the position's current APR; a `yield_withdraw` redeems
principal + accrued back to the chain balance. Correctness hinges on three
things: accrual scaling with *actual* elapsed seconds, money conservation across
deposit/accrue/withdraw, and being honest about what's simplified (no
compounding, 1:1 USD valuation of non-stable assets, USD-as-asset-units gas).

The deposit/withdraw execution lives in
`crates/execution-models/src/yields.rs`; the per-tick accrual and valuation live
in `crates/simulation-engine/src/engine.rs`; the position math lives in
`crates/portfolio-ledger/src/lib.rs` and `.../position.rs`.

## What it is

### The accrual rule (simple interest, per tick)

Every tick, for every open yield position, the engine credits

```
interest = principal · apr · fraction
fraction = elapsed_secs / YEAR_SECONDS        (YEAR_SECONDS = 31_536_000)
```

`crates/simulation-engine/src/engine.rs:1019` (`accrue_yield`):
- `fraction` is computed once per tick from `elapsed_secs`
  (`engine.rs:1028`); `YEAR_SECONDS = 31_536_000` (`engine.rs:27`).
- `elapsed_secs` is `ts - prev_ts` (the actual gap since the previous tick), or
  the configured `interval_secs` on the very first tick when there is no prior
  tick (`engine.rs:241`).
- `apr` is looked up per position via `index.apr_at(key, ts)`
  (`engine.rs:1031`); if there's no APR series for the position at all, that
  position is skipped this tick (`else { continue }`).
- `interest = y.principal · apr · fraction` (`engine.rs:1032`). Note it uses
  **`principal` only** — accrued interest is not part of the base, i.e. it does
  **not compound** (see #114 below).
- Zero interest is skipped (no event); otherwise it calls
  `ledger.accrue_yield(...)` and emits a `yield_accrued` event carrying `apr` and
  `interest_usd` (`engine.rs:1036–1049`).

`ledger.accrue_yield` (`crates/portfolio-ledger/src/lib.rs:233`) adds the
interest to `position.accrued` and to the cumulative `yield_usd` counter
(`lib.rs:248–249`); it errors (`NoSuchYield`) if the position doesn't exist
(`lib.rs:243`), which the engine ignores with `let _`, since it only iterates
over positions it just snapshotted (`engine.rs:1027`).

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

`compute_equity` (`engine.rs:1087`) adds, for every open yield position,
`y.value()` (= principal + accrued) directly into USD equity
(`engine.rs:1108–1109`). It does **not** price the underlying asset — see the
1:1 USD limitation below.

### Policy knobs (and which are honored)

| Policy field | Variants | Honored? |
| --- | --- | --- |
| `yield_accrual` | `SimpleApr`, `CompoundApy`, `ProtocolIndex` | **No — not read.** The engine always does simple APR. |

The `YieldAccrual` enum is defined at
`crates/simulation-policies/src/lib.rs:111` and carried on `ResolvedPolicy`
(`lib.rs:144`), but a grep of `crates/simulation-engine/src/` finds **no
reference** to `yield_accrual` / `CompoundApy` / `ProtocolIndex` / `SimpleApr`.
So `CompoundApy` and `ProtocolIndex` are **enum stubs with no behavior**; every
profile accrues identically as simple APR (#121, and compounding #114).

## Which market / when to use

Yield positions model lending-protocol deposits (Aave-style) on a chain — a way
to park stable capital and earn APR between trades, or to backtest a
yield-bearing leg of a strategy. The APR comes from a per-(protocol, asset,
chain, pool) yield series in the market-data bundle (see the `accrual_gaps.rs`
test bundle for the shape: a `yields` entry with a `points` array of
`{ts, apr}` points).

There is currently only one accrual behavior (simple APR), so there's no
"choose one over another" decision to make at the policy level — the choice that
matters is whether your deposit asset is a stablecoin (valuation is correct) vs.
a volatile asset (valued 1:1 USD, which is wrong — see below).

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
- **NOT compounding (#114).** Interest is `principal · apr · fraction`; the
  `accrued` balance is never folded back into the principal base
  (`engine.rs:1032`). So this is **simple interest**, not APY. The `CompoundApy`
  policy variant exists but is not implemented.
- **No yield-policy gate (#121).** `accrue_yield` is called unconditionally every
  tick (`engine.rs:245`) and never consults `policy.yield_accrual`. There is no
  way to select compounding or a protocol-index model, and no way to turn yield
  accrual off via policy (unlike funding, which has a `Funding::None` knob —
  `crates/simulation-policies/src/lib.rs:101`).
- **Non-stable yield valued 1:1 USD (#115).** `compute_equity` adds
  `y.value()` (a quantity in *asset units*) straight into USD equity
  without multiplying by a mark price (`engine.rs:1108–1109`). For a stablecoin
  deposit (USDC/USDT/DAI; see `is_stable`,
  `crates/execution-models/src/pricing.rs:15`) 1 unit ≈ \$1, so this is fine. For
  a volatile asset (e.g. an ETH or wstETH deposit) the position value is wrong —
  the asset's USD price is ignored. Note that, unlike chain balances (which *are*
  priced via `mark_price` at `engine.rs:1096`), yield positions get no such
  pricing. The accrual itself is also unit-agnostic: `apr` is applied to the
  asset-unit principal.
- **Gas charged in USD but debited as asset units (#115).** `gas_usd` returns a
  USD figure (`crates/execution-models/src/pricing.rs:102`), but both deposit and
  withdraw debit it via `ledger.debit(chain, cfg.asset, gas)`
  (`yields.rs:45`, `yields.rs:86`) — i.e. `gas` USD is subtracted as if it were
  `gas` *units of the deposit asset*. Correct for a stable deposit asset, but for
  a non-stable asset the gas debit is mis-denominated. (`hyperliquid` gas is
  zero, `pricing.rs:103`; `GasModel::HistoricalFeeHistory` falls back to the
  policy's fixed amount when no gas series exists, `pricing.rs:109–111`.)
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
  fires 1h at tick 1 and 2h across the gap at tick 3, totaling the full 3h
  (`10000 · 0.05 · 3·3600/31_536_000 ≈ 0.171233`), proving #118 — the pre-fix
  static-interval value (2h, `0.114155`) is explicitly checked against.

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

(Note: no test exercises a *non-stable* yield deposit, so the 1:1 USD valuation
and asset-unit gas behaviors of #115 are not covered by tests — they are
inferred from the code paths cited above. No test exercises `CompoundApy` /
`ProtocolIndex`, since they have no behavior.)

## Related issues

- [#114](https://github.com/ftvision/catalyst-backtest/issues/114) — yield is simple, not compounding
- [#115](https://github.com/ftvision/catalyst-backtest/issues/115) — non-stable yield valuation (1:1 USD, gas units)
- [#118](https://github.com/ftvision/catalyst-backtest/issues/118) — elapsed-time accrual — FIXED
- [#121](https://github.com/ftvision/catalyst-backtest/issues/121) — no Yield policy gate
