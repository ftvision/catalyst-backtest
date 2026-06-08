# Partial fills & insufficient balance

When an action asks to trade more than the portfolio can afford, the simulator
has to decide: reject the whole thing, fill what it can, or let the balance go
negative. This is a money-conservation question — the wrong answer mints assets
out of thin air. Two policy knobs *describe* the intended behavior
(`balance.insufficient_balance` and `fills.partial_fills`), but the engine today
implements only one path: **reject-and-roll-back**. This doc is explicit about
what is real versus declared-but-unimplemented.

## What it is

Two independent enums in the resolved policy
(`crates/simulation-policies/src/lib.rs:39-47`):

| Knob | Variants | Meaning (as named) | Implemented? |
| --- | --- | --- | --- |
| `insufficient_balance` | `reject` | refuse the action, leave balances unchanged | **yes** |
| | `partial_fill` | fill as much as the balance covers | **no** (no engine code path) |
| | `clamp_to_available` | shrink the trade to the available balance | **no** (no engine code path) |
| | `allow_negative` | permit the balance to go negative | **yes** (disables the ledger's overdraw guard) |
| `partial_fills` | `none` | actions are all-or-nothing | **yes** (the only real behavior) |
| | `allow_if_configured` | partial fill allowed when other knobs ask for it | **no** (inert flag) |
| | `always_allow` | always permit partial fills | **no** (inert flag) |

What the engine actually does, end to end:

1. **Sizing first.** `resolve_amount`
   (`crates/simulation-engine/src/engine.rs:910`) turns a relative `Amount`
   (e.g. `pct_balance`) into an absolute size against the *current* balance. A
   `pct_balance` swap therefore can't overdraw on its own, but absolute amounts,
   fees, and gas still can.
2. **Execute on a trial copy.** `run_action_chain`
   (`crates/simulation-engine/src/engine.rs:648-651`) clones the ledger, runs the
   action against the clone, and **commits the clone only on full success**
   (`ActionOutcome::Executed`). On `Rejected` the real ledger is never touched.
3. **The ledger enforces the floor.** Every spend goes through `Ledger::debit`
   (`crates/portfolio-ledger/src/lib.rs:88-107`). Under the default
   (`allow_negative = false`) it returns `LedgerError::InsufficientBalance`
   *before* mutating anything when `amount > available`
   (`lib.rs:91-98`). The execution model turns that error into a rejection
   (e.g. swap: `crates/execution-models/src/swap.rs:150-152` and `:183-185`;
   perp margin: `crates/execution-models/src/perp.rs:124`; yield gas:
   `crates/execution-models/src/yields.rs:45,86`).

So `insufficient_balance` is wired into exactly one place: `allow_negative`
flips the ledger guard. The engine reads it once at
`crates/simulation-engine/src/engine.rs:201-202`
(`allow_negative = policy.insufficient_balance == InsufficientBalance::AllowNegative`)
and threads the boolean into `initial_ledger` / `Ledger::with_initial`. The
`partial_fill` and `clamp_to_available` variants are **never matched anywhere**
in `simulation-engine` or `execution-models` — they fall through to the same
reject behavior as `reject`, because nothing shrinks the order before `debit`
runs.

`partial_fills` is wholly a **stub**: `PartialFills` is read by nothing except
the validator. The only place it has any effect is a consistency check in
`validate` (`crates/simulation-policies/src/resolve.rs:172-178`), which rejects
the contradictory combo `insufficient_balance = partial_fill` together with
`partial_fills = none`. That guard exists to keep a config from *declaring*
partial fills it can't get; it does not enable partial fills.

### Trial-ledger commit-on-success (atomicity)

The clone-execute-commit pattern (`engine.rs:643-651`) is the core
money-conservation mechanism. Because a single action can debit in several steps
— e.g. a yield deposit moves principal *and* then charges gas separately
(`yields.rs:43-46`) — a failure on a *later* step must not leave the *earlier*
step's mutation behind. Running on a throwaway clone and only swapping it in on
`Executed` makes each action atomic without every model hand-rolling a rollback.
Resting-order fills use the same pattern
(`crates/simulation-engine/src/engine.rs:764-771`).

## Which behavior / when to use

- **`reject` (default, all three profiles' implemented behavior):** the safe,
  conservative choice. `strict_v1` sets `insufficient_balance = reject`,
  `partial_fills = none` (`crates/simulation-policies/src/profiles.rs:13-14`),
  and `conservative_v1` / `research_v1` inherit `insufficient_balance = reject`
  via `..strict_v1()` (`profiles.rs:38-61`). An unaffordable action simply
  doesn't happen and is logged as `action_rejected`. Use this for any result
  you want to trust.
- **`allow_negative`:** lets balances go below zero (the ledger guard is off).
  This models implicit borrowing/leverage at the cash level and **breaks the
  no-overdraw invariant by design** — only use it deliberately, e.g. to study a
  strategy that assumes a credit line. Not set by any profile.
- **`partial_fill` / `clamp_to_available` / `partial_fills != none`:** do **not**
  select these expecting partial execution — the engine has no code for it, so
  the action either fully fills (if affordable after sizing) or is fully
  rejected. `research_v1` sets `partial_fills = allow_if_configured`
  (`profiles.rs:54`), but that flag is inert; it does not change execution.

## Correctness notes / edge cases

- **Money conservation under `reject` / `none`.** A rejected debit mutates
  nothing (`lib.rs:91-98` returns before the subtraction), and the trial copy is
  discarded, so a rejected action conserves the portfolio exactly. Verified by
  the ledger and engine tests below.
- **Multi-step atomicity.** The yield-deposit-then-gas case is the canonical
  trap: principal is affordable, gas is not. Without the trial copy the ledger
  would be left with `0 USDC + a phantom 250 principal position`. The trial
  commit prevents this (see `yield_deposit_failing_on_gas_leaves_ledger_untouched`).
- **Sell where fee + gas exceed proceeds is rejected, not partially filled (#117).**
  A dust sell whose `proceeds - fee - gas <= 0` is rejected *before* any debit
  (`swap.rs:178-182`), rather than crediting a negative `net` (which would mint
  phantom debt in the destination asset). This is a rejection, not a clamp — the
  trade does not shrink to a break-even size.
- **Perp loss is capped at posted margin; settlement floored at 0 (#117).** A
  leveraged perp can't lose more than the margin you posted — close settlement
  is computed as `(returned_margin + realized_pnl - fee).max(0)`
  (`crates/execution-models/src/perp.rs:187-192`), so a blown-up position
  returns nothing rather than producing a negative credit that would overdraw
  collateral.
- **No look-ahead.** Sizing reads the *current* balance only; the
  insufficient-balance decision uses balances as of the tick being processed —
  no future bar is consulted. (Look-ahead avoidance for *prices* is a separate
  concern; see the slippage and price-selection docs.)
- **Determinism.** All three paths (`reject`, `allow_negative`, and the
  unimplemented variants that collapse to reject) are pure functions of the
  ledger state and the order, with no randomness. The `debit` guard is a plain
  `amount > available` comparison on `Decimal` (`lib.rs:91`).
- **Known limitation — `partial_fill` and `clamp_to_available` are declared but
  unimplemented.** Selecting either gives reject semantics, *not* a partial fill.
  No engine code path reads them; treat them as reserved enum values until one
  does. I could not find a tracking issue number in the repo for wiring these up
  — stated as unverified.
- **Known limitation — `partial_fills` is an inert flag.** It influences only the
  `validate` consistency check (`resolve.rs:172-178`); it never reaches the
  engine.

## Tests (executable documentation)

Ledger-level overdraw guard — `crates/portfolio-ledger/tests/ledger.rs`:
- `strict_ledger_refuses_to_overdraw` — `debit` of more than the balance returns
  `InsufficientBalance` and **leaves the balance unchanged** (`ledger.rs:33-39`).
- `allow_negative_ledger_permits_overdraw` — with `allow_negative = true`, the
  debit succeeds and the balance goes negative (`ledger.rs:42-46`).
- `overdraw_yield_is_rejected` — the parallel guard on yield withdrawals
  (`ledger.rs:172`).

Execution-model rejection (and ledger-untouched) —
`crates/execution-models/tests/execution.rs`:
- `buy_with_insufficient_balance_is_rejected_and_ledger_unchanged` — a buy beyond
  the from-asset balance rejects and mutates nothing (`execution.rs:119`).
- `sell_more_than_held_is_rejected` — symmetric on the sell side
  (`execution.rs:129`).
- `dust_sell_where_gas_exceeds_proceeds_is_rejected_not_credited` — the #117
  fee+gas-exceeds-proceeds rejection, no negative credit (`execution.rs:229`).
- `yield_deposit_insufficient_is_rejected` — deposit beyond balance rejects
  (`execution.rs:281`).

Engine-level atomicity / commit-on-success —
`crates/simulation-engine/tests/engine.rs`:
- `yield_deposit_failing_on_gas_leaves_ledger_untouched` — principal affordable,
  gas not; the trial-copy commit means **0 `action_executed`, 1
  `action_rejected`, ledger fully intact** (no phantom position)
  (`engine.rs:238-273`). This is the direct test of the commit-on-success
  mechanism.
- `selling_more_than_held_is_rejected` — end-to-end rejection through the engine
  (`engine.rs:186`).

Policy validation of the declared-but-unimplemented combo —
`crates/simulation-policies/tests/policies.rs`:
- `partial_fill_balance_without_partial_fills_is_rejected` — `strict_v1` (which
  has `partial_fills = none`) plus `insufficient_balance = partial_fill` is a
  config contradiction and fails to resolve (`policies.rs:124-129`). This pins
  the validator guard, not any partial-fill execution.
