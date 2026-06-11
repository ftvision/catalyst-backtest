# Limit orders

A **limit order** is *placed* (validated against the ledger) on one bar but does
not fill until a *later* bar's price touches its limit — or it expires unfilled.
This is the system's only deferred-execution path, so its correctness hinges on
two things being right: it must never use information from a bar it couldn't have
known at placement time (look-ahead), and a resting limit must fill at a maker
price (no taker slippage). The order's `order_type` field selects the limit path
(`order_type == "limit"`); `limit_price` parameterizes it, and `time_in_force` /
`expire_after_bars` control how long it rests.

Two layers own the behavior:
- **`crates/execution-models/src/limit.rs`** — the instrument-independent
  decisions: direction (`place_swap_limit` / `place_perp_limit`) and the
  gap-aware touch + fill price (`limit_fill_price`).
- **`crates/simulation-engine/src/engine.rs`** — the resting book, next-bar
  eligibility, time-in-force expiry, and lifecycle events (`RestingOrder`,
  `fill_resting_orders`, `resolve_expiry`).

## What it is

### Direction (who rests where)
A limit's `LimitSide` (`limit.rs:27`) is `Buy` or `Sell`. Buys rest *below* the
market and fill when price falls to them; sells rest *above* and fill when price
rises.

| Order | Side resolved by | Rule |
| --- | --- | --- |
| Swap, stable → asset | `place_swap_limit` (`limit.rs:86`) | `Buy` (acquiring the asset) |
| Swap, asset → stable | `place_swap_limit` (`limit.rs:87`) | `Sell` |
| Swap, neither/both stable | `place_swap_limit` (`limit.rs:88`) | **Rejected** — exactly one side must be a stable |
| Perp long open | `place_perp_limit` (`limit.rs:131`) | `Buy` |
| Perp short open | `place_perp_limit` (`limit.rs:132`) | `Sell` |
| Perp reduce-only, position long | `place_perp_limit` (`limit.rs:119`) | `Sell` (take-profit closes the long) |
| Perp reduce-only, position short | `place_perp_limit` (`limit.rs:120`) | `Buy` |
| Perp reduce-only, **no** position | `place_perp_limit` (`limit.rs:122`) | **Rejected** |
| Any limit with no/zero `limit_price` | `limit_price` (`limit.rs:69`) | **Rejected** at placement |

### Touch + gap-aware fill price
`limit_fill_price(bar, side, limit)` (`limit.rs:62`) decides, per bar, whether an
order fills and at what price:

```
Buy:  (bar.low  <= limit).then(|| bar.open.min(limit))
Sell: (bar.high >= limit).then(|| bar.open.max(limit))
```

- A buy fills iff the bar's **low** reached the limit; a sell iff the **high** did.
- **Gap-aware**: if the bar *opens through* the limit (a buy whose open is already
  below the limit, or a sell whose open is above), it fills at the **open** — the
  better, more favorable price the trader would actually have gotten — not at the
  stale limit. `open.min(limit)` for buys and `open.max(limit)` for sells encode
  exactly this: never worse than the limit, better on a gap-through
  (`limit.rs:64`–`65`).

### Resting, eligibility, and expiry (engine)
On placement, the action produces an `ActionOutcome::Resting`; the engine emits an
`order_placed` event and pushes a `RestingOrder` recording `placed_index` (the
placement tick) and `expire_after_bars`. Placement does **not** move any balance —
but it does **reserve** the fill's exact spend in the ledger's reservation side
table (#124, see "Balance reservations" below), and any downstream graph actions
are deferred until the order actually fills (run at the fill bar).

Each tick, before any new actions run, `fill_resting_orders` scans the book. It
runs **after** `fill_pending_market` (the deferred `next_open` market fills, #116):
deferred market orders fill at the bar's **open** (the earliest price of the bar),
resting limits at a later intra-bar **touch**, so when a reduce-only take-profit
limit and a deferred market entry land on the same bar the entry opens first and
the limit can then reduce it. (A reduce-only limit therefore can't be *placed* until
its position exists — under `next_open` that means it chains off the entry's fill on
the next bar, not the decision bar.) The scan itself:

1. **Next-bar eligibility** (`engine.rs:734`): `if order.placed_index >= tick_index`
   the order is skipped and kept. An order placed at tick *T* is first eligible at
   *T+1*. The placement bar is never inspected for a fill.
2. **Time-in-force** (`engine.rs:739`): if `expire_after_bars == Some(n)` and
   `tick_index > placed_index + n`, the order expires (`order_expired`, reason
   `"time_in_force elapsed"`) *before* a fill is attempted on that too-late bar.
   `None` (GTC) never expires this way.
3. **Touch**: look up the bar for `(venue, symbol)`; if absent, keep the order for
   a later bar (`engine.rs:754`). Otherwise call `limit_fill_price`; on `None`,
   keep and wait (`engine.rs:759`).
4. **Fill** at the touched price via `execute_swap_at` / `execute_perp_at`
   (`engine.rs:766`–`767`), atomically on a cloned ledger that is committed only on
   success (`engine.rs:771`). Emits `order_filled` with the fill plus `order_id`
   and `limit_price`; a model-level rejection emits `order_rejected` and leaves the
   ledger untouched.

Any order still resting when the run ends expires with reason
`"backtest ended with order resting"` (and its reservation is released).

### Balance reservations (#124, fixed)

A resting order **earmarks the balance its fill will spend** so nothing else can
spend it in the meantime. The primitive lives in the ledger
(`crates/portfolio-ledger/src/lib.rs`): `reserve(order_id, venue, asset, amount)`
records a `Reservation` in a side table keyed by order id; `release(order_id)`
removes it (idempotent); `reserved(venue, asset)` sums the earmarks and
`available(venue, asset) = balance − reserved` is the spendable figure. The
strict `debit` overdraw guard compares against **available**, and its rejection
names the earmark: `"need X, have Y available (Z reserved by resting orders)"`.

**A reservation is an earmark, NOT a debit.** The balance — and therefore the
portfolio snapshot and equity — is untouched by placement: owned-but-committed
cash is still owned, and still equity (see
[portfolio-valuation](portfolio-valuation.md)). `to_portfolio`/valuation never
read reservations. (Historical note: the original issue text claimed equity
"over-counts" an open order's cash — that framing was wrong; the real bug was
that the committed cash stayed *spendable*, enabling double-commitment and
silent starvation of the resting fill.)

**What each order type reserves** — the exact, price-independent amount its
fill will debit (maker fills pay no slippage, #162), computed by
`swap_reservation` / `perp_reservation` (`limit.rs`):

| Order | Fill debits | Reserved |
| --- | --- | --- |
| Swap buy (stable→asset) | `amount + fee + gas` of `from_asset` | the same sum (`fee = fee_bps·amount`; gas estimated at the placement bar) |
| Swap sell (asset→stable) | exactly `amount` base units (fee+gas come out of proceeds) | `amount` |
| Perp limit open | `margin + fee` USDC (`margin = size_usd/leverage`) | the same sum |
| Perp reduce-only limit | nothing (credits only) | nothing |

**Lifecycle**:
- **Placement validates and reserves.** `place_swap_limit`/`place_perp_limit`
  compute the reservation and reject when it exceeds the venue's *available*
  balance — an unaffordable order is rejected at placement (`action_rejected`)
  instead of resting doomed and silently failing at fill (a deliberate behavior
  change). On success the engine records the reservation on the real ledger
  under the minted order id and echoes it in the `order_placed` event detail
  (`reserved_asset`/`reserved_amount`, additive fields; null for reduce-only).
- **Every exit from the book releases.** `fill_resting_orders` releases the
  reservation on the real ledger *before* attempting the fill (the fill spends
  the very funds the earmark was holding), on TIF expiry, and the run-end drain
  releases leftovers.
- **Fill-time starvation is loud.** With the spend validated at placement, a
  fill rejection is exceptional — the one drift vector is **historical gas**
  between placement and fill (the reservation froze the placement-bar gas
  estimate). A rejected fill emits `order_rejected` *and* a run-level warning
  (`"resting order {id} starved at fill: …"`), and the reservation is released.
- **Deferred `next_open` market orders (#116) use the same mechanism**: the
  deferral reserves at the decision bar (amounts are already resolved there;
  the `"all"` sentinel is **frozen to an absolute amount at the decision bar**
  for any queued order, since the reservation needs an exact figure), the
  `order_deferred` detail carries the same `reserved_*` fields, an unaffordable
  deferral rejects at the decision bar, and the fill/lapse releases — so two
  same-bar deferred orders can't both commit the full balance.
- **Sizing reads available.** `pct_balance` bases in the engine and the `"all"`
  sentinel paths in `swap.rs`/`yields.rs` read `available(venue, asset).max(0)`
  instead of the raw balance (see [sizing](sizing.md)).
- **`allow_negative` keeps reservations inert**: `reserve` never fails and the
  debit is unguarded, preserving that policy's explicit-debt model (only the
  #165 sign guard still applies).

### Time-in-force resolution
`resolve_expiry(time_in_force, expire_after_bars)` (`engine.rs:115`):

| `time_in_force` | Result | Meaning |
| --- | --- | --- |
| `"gtc"` | `None` | good-til-cancelled — only expires at run end |
| `"good_til_bars"` | `expire_after_bars` | good-til-`n`-bars |
| absent | `expire_after_bars` (as-is) | implicit good-til-`n`-bars if a count is set, else GTC-like `None` |

`time_in_force` is a **closed enum validated at graph compile time** (#160,
fixed): the compiler rejects unknown TIF strings (a `"gct"` typo is a compile
error naming the node, no longer a silent fall-through to the
`expire_after_bars` path), `"good_til_bars"` without `expire_after_bars` (it
used to silently mean GTC), and `"gtc"` *with* `expire_after_bars`
(contradictory — the expiry used to be silently ignored). Absent TIF with
`expire_after_bars` set remains legal as implicit good-til-bars.
`resolve_expiry` keeps a conservative fallback arm (honor `expire_after_bars`)
for unknown strings, which are unreachable post-validation.

## Which market / when to use

Limit orders apply to both **swaps** (DEX, one stable leg) and **perps** (e.g.
Hyperliquid). Use them whenever the strategy wants execution conditional on price
rather than immediate market fills:
- **Entry at a better price** — rest a buy below / sell above the current market
  and only trade if price comes to you.
- **Reduce-only take-profit** — a reduce-only perp limit closes an existing
  position at a target; it requires an open position and inherits the closing
  direction (`limit.rs:116`–`134`).

GTC vs good-til-`n`-bars is the patience knob: GTC rests until filled or run end;
good-til-`n`-bars caps how long a stale order lingers before being cancelled.

## Correctness notes / edge cases

- **No look-ahead.** The placement bar is never used for a fill: an order placed at
  tick *T* is eligible only from *T+1* (`engine.rs:734`). A bar that dips through
  the limit on the very bar the order was placed does **not** fill — verified by
  `limit_does_not_fill_on_placement_bar`. This is the central anti-look-ahead
  guarantee: you can't fill against information from the bar you submitted on.
- **Gap-through is favorable, not adverse.** A buy that gaps open below its limit
  fills at the (lower) open, a sell that gaps above fills at the (higher) open —
  the trader is never worse off than the limit, and benefits from a gap in their
  favor (`limit.rs:64`–`65`). This avoids the unrealistic outcome of filling a
  resting maker at a stale, unfavorable limit when the market clearly traded
  through it.
- **No taker slippage on resting fills — maker semantics, decided (#162).** A
  resting limit is a **maker** order: it fills at **limit-or-better**, exactly at
  the gap-aware touched price, with **no bps/volume slippage and no AMM price
  impact applied**. `execute_perp_at` (`perp.rs:68`) and `execute_swap_at`
  (`swap.rs:117`) take the price as given and never call the slippage path —
  under `amm_price_impact` *with a pool-reserves series present*, the
  constant-product model is **not** re-run on the limit price (it previously was,
  which could fill a buy limit at 1900 *above* 1900 — the #162 bug). This matches
  how real on-chain limit orders execute: Uniswap v3 range orders and 1inch limit
  orders are maker liquidity that fills at the placed price, not taker swaps
  against the pool. For honesty, the theoretical constant-product price the trade
  *would* have paid as a taker is still computed and emitted in the fill detail of
  swap limit fills: `amm_theoretical_price`, plus `amm_impact_exceeds_limit`
  (true when the theoretical price is worse than the actual fill from the
  trader's perspective — i.e. the pool was too shallow to honestly fill that size
  at the limit). Escalation path if trigger+market semantics (a stop-style
  "trigger touches, then swap against the pool with impact") are ever wanted: a
  future `fills.limit_fill_model = maker | trigger_market` policy knob — documented
  here as the named extension point, **not built**.
- **Fees and gas still apply on the fill.** "No slippage" refers to price only.
  The committed fill still charges trading fees and gas through `swap_at`
  (`swap.rs:142`–`188`) / `open_perp` / `close_perp` — and inherits their
  money-conservation guards: a swap sell whose fee+gas exceed proceeds is rejected
  rather than minting phantom debt (`swap.rs:178`, #117), and a leveraged perp's
  loss is capped at posted margin on close — the settlement is floored at zero
  (`perp.rs:192`, #117). The fill is atomic: a rejected fill leaves the real ledger
  untouched (`engine.rs:764`, `771`).
- **Expiry is checked before the fill attempt** on the expiring bar
  (`engine.rs:739` precedes the touch at `engine.rs:758`): an order at its
  good-til-`n` boundary that *also* would touch on the next bar still expires —
  the engine does not give a one-bar grace fill past the TIF. With
  `expire_after_bars = 1`, an order placed at tick 0 is eligible only on tick 1 and
  expires on tick 2 (`tick_index 2 > placed_index 0 + 1`).
- **Determinism.** The book is a `Vec<RestingOrder>` scanned in insertion order via
  `std::mem::take` + rebuild (`engine.rs:729`), order ids are a monotonic counter
  (`engine.rs:679`–`680`), and ticks are data-driven and sorted — so fills and
  expiries are reproducible. Missing-bar handling keeps the order for a later bar
  rather than dropping it (`engine.rs:754`).
- **Reduce-only requires a live position** at placement, resolved against the
  ledger (`limit.rs:117`); placement is rejected if none exists, and direction is
  the closing direction of the position (long → sell TP, short → buy).
- **Downstream chaining only on fill.** Graph edges out of a limit node run only
  when the order fills, and they run at the *fill* bar with that bar's context
  (`engine.rs:788`–`793`), never at placement.

## Tests

`crates/simulation-engine/tests/limit_orders.rs` (end-to-end through `run`):
- `perp_limit_rests_then_fills_when_touched` — placed bar 0, fills bar 1 at exactly
  `1900` (no slippage), `order_placed`/`order_filled` counts, fill `ts` is the fill
  bar not the placement bar (and no `action_executed` for the limit perp itself).
- `limit_does_not_fill_on_placement_bar` — bar 0 dips through the limit but the
  order is ineligible there; it never fills and expires GTC at run end. The core
  no-look-ahead test.
- `limit_gap_through_fills_at_open` — bar 1 gaps open below the buy limit (open
  `1850`); fills at the open `1850`, not the `1900` limit.
- `limit_expires_after_n_bars` — `time_in_force = "good_til_bars"`,
  `expire_after_bars = 1`: never touches, expires on bar 2 with reason
  `"time_in_force elapsed"`.
- `reduce_only_limit_take_profit_fills` — market long opened, reduce-only sell limit
  at `2200` fills when bar 1's high reaches it; position closes out.
- `swap_limit_fills_when_touched` — swap buy limit fills at `1900`, crediting ETH.
- `limit_without_price_is_rejected` — missing `limit_price` → `action_rejected`, no
  `order_placed`.

`crates/simulation-engine/tests/issue_162_amm_limit_fill.rs` (maker semantics
under `amm_price_impact`, end-to-end through `run`):
- `buy_limit_shallow_pool_fills_at_limit_with_honest_impact_fields` — shallow pool,
  theoretical impact price above the limit: fills AT the limit; the fill detail
  carries `amm_theoretical_price` > limit and `amm_impact_exceeds_limit = true`.
- `buy_limit_deep_pool_still_fills_at_maker_price_not_favorable_amm` — pins
  maker-not-clamp: a *favorable* theoretical AMM price is not substituted either.
- `sell_limit_shallow_pool_fills_at_limit_with_honest_impact_fields` — sell mirror.
- `gap_through_with_reserves_fills_at_open` — gap-aware limit-or-better survives
  with reserves present; no AMM override of the open fill.
- `market_swaps_still_get_amm_impact` — regression: the market path keeps depth
  impact (companion to `amm_slippage.rs`).

`crates/simulation-engine/tests/issue_124_resting_reservation.rs` (reservation
lifecycle, end-to-end through `run`):
- `placement_does_not_move_equity_or_balances` — earmark-not-debit: equity and
  the portfolio are identical across the placement bar.
- `market_order_cannot_spend_reserved_cash` — a later market order is rejected,
  naming the reserved figure.
- `pct_balance_sizes_against_available_not_raw_balance` — 50% of (1000 − 600
  reserved) = 200.
- `fill_releases_the_reservation_for_its_own_spend_and_downstream` /
  `expiry_releases_the_reservation` — release on fill and TIF expiry.
- `gas_drift_starves_the_fill_with_run_warning_and_releases` — the gas-drift
  shortfall vector: `order_rejected` + run warning + release.
- `second_overcommitting_limit_rejected_at_placement` — placement validation.
- `allow_negative_keeps_reservations_inert` — the inert mode pin.
- `perp_limit_open_reserves_margin_plus_fee` /
  `perp_reduce_only_limit_reserves_nothing` — perp reservation amounts.
- `same_bar_deferred_markets_cannot_both_spend_the_balance` /
  `deferred_all_freezes_at_the_decision_bar` — #116 deferred orders reserve
  too, with `"all"` frozen at the decision bar.

`crates/portfolio-ledger/tests/ledger.rs` covers the primitive
(reserve/release/available, the debit guard reading available, clone travel,
inert `allow_negative`, negative-amount guard).

`crates/execution-models/tests/execution.rs` (unit-level, `limit.rs`):
- `buy_limit_touches_when_low_reaches_it` / `sell_limit_touches_when_high_reaches_it`
  — touch only when low/high reaches the limit; `None` otherwise.
- `buy_limit_gap_through_fills_at_open` and the sell gap case inside
  `sell_limit_touches_when_high_reaches_it` (a gap up fills at the `2250` open) —
  gap-through fills at the open.
- `place_swap_limit_resolves_side_and_rejects_bad_input` — stable→asset is a buy,
  asset→stable a sell, missing price / non-stable pair rejected.
- `place_perp_limit_open_long_is_a_buy` — long open resolves to `Buy`.
- `place_reduce_only_limit_requires_a_position_and_closes_it` — reduce-only rejected
  with no position, and resolves to the closing side when one exists.
- `swap_limit_buy_reserves_amount_plus_fee_plus_gas_and_validates_available` /
  `swap_limit_sell_reserves_exactly_the_base_amount` /
  `perp_limit_open_reserves_margin_plus_fee_and_reduce_only_reserves_nothing` /
  `placement_validation_is_inert_under_allow_negative` — #124 reservation
  amounts and placement-time availability validation.

## Related issues

- [#124](https://github.com/ftvision/catalyst-backtest/issues/124) — resting orders reserve balance — FIXED (earmark side table, see "Balance reservations" above; note the issue's "equity over-counts" framing was wrong — the bug was double-spend/starvation)
- [#162](https://github.com/ftvision/catalyst-backtest/issues/162) — AMM price impact repriced resting swap limit fills — FIXED (maker semantics, see above)
