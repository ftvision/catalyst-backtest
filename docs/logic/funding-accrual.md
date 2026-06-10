# Perp funding accrual

**Funding** is the periodic cash flow a perpetual-futures venue charges between
longs and shorts to tether the perp price to spot. In the backtest it is the
recurring carry cost of holding a perp position: every tick, each open perp pays
(or receives) `rate · notional` in USDC. Getting its **sign**, its **notional
basis**, and especially its **time scaling** right is what makes a leveraged-carry
backtest honest — a funding bug silently rewards or penalizes every held bar.

Driven entirely by the `perps.funding` policy field; accrual happens in the
engine's per-tick loop, before any actions run.

## What it is

Accrual lives in `crates/simulation-engine/src/engine.rs` — `accrue_funding`
(`engine.rs:970`), called once per tick at the top of the loop
(`engine.rs:244`), before resting-order fills and signal/action evaluation.

For each open perp position `p`:

```
rate     = funding_sum(venue, symbol, ts - elapsed_secs, ts)   // (engine.rs:988)
mark     = mark_price(venue, symbol, ts)                        // close, else last price
notional = p.size · mark                                        // (engine.rs:993)
sign     = +1 if Long, -1 if Short                             // (engine.rs:994-997)
payment  = sign · rate · notional   // positive = WE PAY        // (engine.rs:998)
```

The payment is then applied as a USDC balance change and recorded. Receipts
(`payment < 0`) are a plain credit; charges (`payment > 0`) go through the
**shortfall cascade** under strict policy (see below, #165):

```
payment < 0          → credit(venue, "USDC", -payment)        // receive
payment > 0, allow_negative → debit(venue, "USDC", payment)   // overdraw = explicit debt
payment > 0, strict  → cash → margin → forgive cascade        // never overdraws
ledger.record_funding(collected)   // signed running total of funding that MOVED money
```

and a `funding_applied` event is emitted carrying the summed `rate`, the owed
`payment_usd`, and `collected_usd` (the part that actually moved cash or
margin; differs from `payment_usd` only when the cascade forgave a remainder
at true bankruptcy). The reporter sums `collected_usd` into
`total_funding_usd`, so reported funding reconciles with cash movements.

`p.size` is base units and **always non-negative**; direction is carried by
`p.side` (`crates/portfolio-ledger/src/position.rs:24-35`), so the `sign` factor
is the only thing that flips the cash-flow direction.

### Policy options

`perps.funding` resolves to the `Funding` enum
(`crates/simulation-policies/src/lib.rs:99-102`). It has exactly two variants:

| Option | Behavior | Status |
| --- | --- | --- |
| `historical` | Accrue real funding from the `funding` data series, as above. The default in every shipped profile (e.g. `strict_v1` sets `Funding::Historical`, `crates/simulation-policies/src/profiles.rs:31`). | implemented |
| `none` | Skip funding entirely — `accrue_funding` returns immediately at the policy guard (`engine.rs:979-981`). | implemented |

There is no modeled/constant-rate or predicted-funding option; `historical`
means "whatever the loaded `funding` series contains," and `none` means zero.

## Sign / direction — longs pay positive funding

The convention is **positive funding rate ⇒ longs pay shorts** (the perp trades
above spot, so longs are charged to close the gap). The code encodes this
directly: `sign = +1` for a long, `-1` for a short (`engine.rs:994-997`), and
`payment = sign · rate · notional` where **positive `payment` means we pay**
(comment, `engine.rs:998`).

- Long + positive rate → `payment > 0` → USDC is debited (the cascade below). Long pays.
- Short + positive rate → `payment < 0` → `credit(-payment)` raises USDC. Short receives.
- A negative rate flips both (longs receive, shorts pay).

`record_funding` stores the signed total (positive = paid by us; comment at
`crates/portfolio-ledger/src/lib.rs`), so the reporter can show net
funding paid vs. received. The recorded amount is funding actually
*collected* — a remainder the cascade forgave at true bankruptcy never moved
money and is excluded (#165).

## Notional basis — marked, not entry

Funding is charged on **current mark notional** `p.size · mark`
(`engine.rs:993`), not entry notional. `mark` is the position's close price this
tick, falling back to the last-known price for the symbol
(`mark_price`, `engine.rs:966-968`). This matches venue behavior: funding scales
with the position's present value, so as the mark moves the per-tick charge
moves with it. If no mark is available this tick the position is skipped for
funding (`engine.rs:992`) — no charge is invented from stale data beyond what
`mark_price`'s last-price fallback already provides.

## Intra-bar SUM over `(ts - elapsed, ts]` — elapsed-time scaling (#118)

This is the correctness core. The tick clock is **data-driven** and can be
coarser than the funding interval (e.g. 4h candle ticks over hourly funding) or
gapped. Two mechanisms keep accrual whole:

1. **Sum, don't sample.** `rate` comes from `funding_sum(venue, symbol,
   ts - elapsed_secs, ts)` (`engine.rs:988`), which sums **every** funding point
   in the half-open interval `(ts - elapsed_secs, ts]`
   (`crates/simulation-engine/src/market.rs:163-173`, using `Bound::Excluded(lo)`
   / `Bound::Included(hi)`). So a 4h tick over hourly funding accrues all four
   hourly points, not just the one landing exactly on `ts`. The half-open shape
   matters: the lower bound is **exclusive** so the point consumed at the end of
   the previous bar is not double-counted, and the upper bound is **inclusive**
   so the point at `ts` is counted exactly once.

2. **Elapsed seconds, not a fixed interval.** `elapsed_secs = ts - prev_ts`, the
   actual gap since the previous tick (`engine.rs:241`), with the configured
   interval used only on the first tick where there is no prior tick (and no
   positions yet) (`engine.rs:230-242`). A gapped clock therefore covers the
   whole real elapsed window rather than one nominal interval. This is the #118
   fix; the same `elapsed_secs` drives yield accrual (`accrue_yield`,
   `engine.rs:1019-1028`).

Note that unlike yield, funding's magnitude is **not** further multiplied by an
elapsed-time fraction. The funding *rate* is already per-funding-interval and
the time dimension is captured by *which* points fall in the window — summing
them is the time scaling. Yield instead scales a continuous APR by
`elapsed_secs / YEAR_SECONDS` (`engine.rs:1028`). Conflating the two would
double-count time for funding.

If the summed `rate` is zero (no funding points in the window, or they net to
zero) the position is skipped before computing a payment (`engine.rs:989-991`),
and a zero payment is likewise skipped (`engine.rs:999-1001`) — no empty events.

## Where the payment lands — the shortfall cascade (#165)

The cash flow targets the position's **own venue `USDC` balance**. Funding is
genuinely owed — it can't be "rejected" like an order — but the ledger no
longer accepts the negative credit that used to let a large charge silently
overdraw the balance even under `strict_v1` (`Ledger::credit` now rejects
negative amounts, `crates/portfolio-ledger/src/lib.rs`). The charge side is an
explicit, policy-dependent decision in `accrue_funding`:

- **Receive** (`payment < 0`): plain `credit(venue, "USDC", -payment)`.
- **Pay under `allow_negative`**: `debit(venue, "USDC", payment)` — the
  overdraw guard is off, the balance goes negative, and the full charge is
  collected as explicit margin debt. This is the historical behavior, kept as
  the opt-in debt model.
- **Pay under strict** (`insufficient_balance = reject`, every shipped
  profile's default): the **cash → margin → forgive cascade**:

  ```
  paid_cash   = min(payment, max(balance, 0))   // never overdraws
  shortfall   = payment - paid_cash
  from_margin = min(shortfall, p.margin_usd)    // position pays from its own collateral
  forgiven    = shortfall - from_margin         // non-zero only at true bankruptcy
  record_funding(payment - forgiven)            // only money that moved
  ```

  When `shortfall > 0` a **`funding_shortfall` event** is emitted with detail
  `{venue, symbol, payment, paid_cash, from_margin, forgiven}`. The margin
  deduction (via `set_perp`) tightens the position's liquidation price, and
  because `check_liquidations` runs immediately after `accrue_funding` in the
  tick loop, a maintenance breach (mmr default `0.0125`, #120) liquidates the
  position the **same tick**, settling whatever residual margin remains. A
  position whose reduced margin stays above maintenance simply survives with a
  tighter `p_liq`.

Positions iterate in `BTreeMap` `(venue, symbol)` order, so when two positions
on one venue are charged in the same tick, the later one deterministically sees
the cash the earlier one drained (and cascades into its own margin if needed).

## Correctness notes / edge cases

- **Look-ahead: none.** Everything read is at-or-before `ts`: funding points in
  `(ts - elapsed, ts]` are all ≤ `ts`, and `mark_price` reads this tick's close
  or an earlier last price. No future bar is consulted.
- **Ordering within a tick.** Funding accrues *first* (`engine.rs:244`), before
  yield, liquidation checks, resting fills, and actions. So funding is charged on
  the position as it stood entering the bar; a position opened later in the same
  tick pays its first funding on the *next* tick. The `funding_interval` test
  confirms this: a position opened at tick 0 accrues nothing at tick 0 and its
  first funding at tick 1 (test comment, `funding_interval.rs:84-85`).
- **Money conservation.** The model is one-sided: it debits/credits the
  position holder's USDC but does **not** credit a counterparty — funding is
  treated as an external cost/rebate, not a transfer between two simulated
  accounts. This is correct for a single-account backtest (you only model your
  own side) but is not a closed two-party conservation invariant.
- **Determinism.** Pure arithmetic on `rust_decimal::Decimal` over a
  `BTreeMap`-ordered funding series (`market.rs:167-173`); positions are iterated
  from a snapshot `Vec` cloned at the top (`engine.rs:982`). No floats, no
  hashing-order dependence — bit-reproducible.
- **Fallbacks.** Absent funding series ⇒ `funding_sum` returns `Decimal::ZERO`
  (`market.rs:172`) ⇒ position skipped, no charge. Absent mark ⇒ position skipped
  (`engine.rs:992`). Neither invents a charge.
- **`Funding::None`** disables accrual wholesale (`engine.rs:979-981`); useful to
  isolate price PnL from carry.

### Known limitations

- Only `historical` and `none` exist — no constant/modeled funding rate option
  (`lib.rs:99-102`).
- No counterparty leg (see money-conservation note above).

(Funding *does* interact with margin and liquidation since #165: a strict-policy
shortfall deducts from the position's margin, and the maintenance check the same
tick can liquidate it. Under `allow_negative` it remains a free-cash-only flow.)

## Tests

`crates/simulation-engine/tests/funding_interval.rs`:

- `four_hour_tick_sums_all_hourly_funding_in_the_bar` — the headline #118 case.
  4h candle ticks (flat 2000) over hourly funding of `0.001`; a 1000-USD long
  opened at tick 0 accrues **nothing** at tick 0, then at tick 1 accrues hours
  1–4 summed to `rate = "0.004"` (asserted on the event detail,
  `funding_interval.rs:90`) with `payment_usd` in `[3.9, 4.0)` — roughly 4× the
  pre-fix single-point value (~1.0), demonstrating the sum-over-the-bar behavior
  and (via the positive `payment_usd`) the long-pays-positive sign
  (`funding_interval.rs:93-94`).

(The test exercises the `Long`/positive-rate path; the short receive-side is
asserted in `issue_165_funding_shortfall.rs` below.)

`crates/simulation-engine/tests/issue_165_funding_shortfall.rs` — the cascade:

- `shortfall_cascades_from_cash_into_margin` — charge 5 against cash 2: cash
  exactly 0, margin 100 → 97, event `{payment 5, paid_cash 2, from_margin 3,
  forgiven 0}`, position survives.
- `zero_cash_pays_funding_entirely_from_margin` — `paid_cash` 0 edge.
- `margin_deduction_breaching_maintenance_liquidates_same_tick` — charge 92
  leaves margin 10 < 12.5 maintenance; liquidated at the same tick's ts,
  residual 10 settled.
- `bankrupt_funding_forgives_remainder_and_liquidates_with_zero_residual` —
  charge 150 vs cash 2 + margin 100: `forgiven` 48, `collected_usd` 102,
  same-tick liquidation settles 0; the whole initial balance reconciles.
- `second_position_same_tick_sees_drained_cash` — deterministic BTreeMap order;
  only the later position falls short.
- `allow_negative_policy_still_overdraws_instead_of_cascading` — regression pin
  for the explicit debt model (balance −3, margin intact, no shortfall event).
- `receiving_funding_still_credits_cash` — the receive side is untouched.

## Related issues

- [#118](https://github.com/ftvision/catalyst-backtest/issues/118) — accrual over actual elapsed time — FIXED
- [#165](https://github.com/ftvision/catalyst-backtest/issues/165) — funding shortfall is an explicit cascade (cash → margin → forgive), never a silent overdraw — FIXED
