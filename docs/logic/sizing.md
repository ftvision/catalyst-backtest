# Position sizing (amount basis)

**Sizing** decides *how many units* an action trades: a swap's from-asset amount,
a perp's USD notional, a yield deposit/withdraw amount. An action's `amount`
(or a perp's `size_usd`) is either an **absolute** quantity, the **`"all"`**
sentinel, or a **relative** percentage of some basis. Getting the basis→units
resolution right is a correctness concern: a wrong basis (or a stale equity
read) silently over- or under-trades, and a perp sized off mark instead of
entry would drift the intended notional.

## What it is

`Amount` is the value type (`crates/contracts/src/graph.rs:88-111`). It is
`#[serde(untagged)]`, so a bare JSON string deserializes to `Absolute` (existing
graphs unchanged) and an object `{ "basis": ..., "value": ... }` to `Relative`:

```
enum Amount {
    Absolute(Decimal),                          // a decimal string, or the "all" sentinel
    Relative { basis: AmountBasis, value: Decimal },
}
```

(Here `Decimal` is the contracts-crate alias for `String` —
`crates/contracts/src/lib.rs:17` — so `Absolute` holds the raw decimal/`"all"`
string and is parsed later.) `Amount::is_all()` (`graph.rs:100-102`) matches the
literal string `"all"`. `AmountBasis` (`graph.rs:77-86`) has three variants:

| Basis | Resolves against | Used by |
| --- | --- | --- |
| `pct_balance` | the relevant asset's **available** balance — balance minus resting-order reservations, clamped at zero (#124) — (swap from-asset, yield asset, perp cash/USDC) | swap, yield, perp |
| `pct_position` | the relevant open position notional/principal | perp, yield (**rejected** for swaps — see below) |
| `pct_portfolio` | total portfolio equity in USD (tick-start) | all |

Relative amounts are resolved to **absolute** *before* execution, by the engine,
in `resolve_amount` (`crates/simulation-engine/src/engine.rs:910-936`):

```
pct = value / 100
PctBalance   -> pct * balance
PctPosition  -> pct * position
PctPortfolio -> pct * equity / unit_price   (rejects if unit_price == 0)
```

The bases (`balance`, `position`, `equity`, `unit_price`) are supplied per
subtype in `execute_action` (`engine.rs`). Since #124 every `balance` base is
the **available** balance — `ledger.available(venue, asset).max(0)`, i.e. the
raw balance minus amounts reserved by resting/deferred orders — so sizing never
counts cash already committed elsewhere:

- **swap**: `pct_position` is **rejected up front** —
  a swap has no position to size against (#121, fixed; previously it silently
  aliased `pct_balance`, hiding user mistakes). The rejection sits before the
  market/limit branch, so resting limit swaps are covered too.
  `balance = ledger.available(chain, from_asset).max(0)` resolves `pct_balance`;
  `unit_price` is the from-asset **mark** (`asset_price`, 1 for stables; see
  "Unit price" below) so `pct_portfolio` converts the USD slice back into
  from-asset units.
  Note the rejection is at **fire time** (an `action_rejected` trace event per
  firing), not graph validation — the run still completes.
- **perp**: `balance = ledger.available(chain, "USDC").max(0)`;
  `position = (p.size * p.entry_price).abs()` — the open position's **entry**
  notional; `unit_price = Decimal::ONE` because `size_usd` is
  already USD (so `pct_portfolio` needs no conversion).
- **yield** (`resolve_yield_amount`): `balance =
  ledger.available(chain, asset).max(0)`; `position = principal + accrued` of the
  matching yield position; `unit_price` is the asset mark (1 for
  stables).

### Unit price: the bounded venue-scoped mark (#119(d), fixed)

`unit_price` for non-perp assets comes from `asset_price` (engine.rs): 1 for
stables, else `MarketContext::mark_close(venue, asset)`, else 0 (which the
`pct_portfolio` guard rejects — see below). The engine's `TickContext`
implements `mark_close` as `close_at(venue, symbol, ts, max_mark_staleness)`
(`crates/simulation-engine/src/market.rs`) — the **same bounded, venue-scoped
carry-forward** that equity valuation's `mark_price` uses (see
[portfolio-valuation](portfolio-valuation.md)). Sizing and equity therefore
always agree about whether an asset is priced:

- **On a gap bar** (the venue has no candle at this tick) the last in-bound
  close is carried forward, so a `pct_portfolio` action sizes at the same mark
  equity used. Under `next_open` this is a real improvement, not just
  consistency: the signal sizes at the carried mark, the market order defers
  (#116), and it legitimately fills at the *next* bar's open — a real price.
- **Filling is gated separately.** Sizing with a mark never lets you *trade*
  at one: under same-bar selections `execute_swap` still rejects on a gap bar
  ("no price for X on Y" — the exact-bar fill guard), and a deferred
  `next_open` order only fills when a real next bar arrives. You can size with
  a mark; you can't fill at one.
- **The staleness bound applies.** With `data.max_mark_staleness` configured,
  an expired mark makes `mark_close` return `None` → `asset_price` returns 0 →
  the `pct_portfolio` zero-price guard rejects, at the same tick the holding
  drops out of equity (#119(b)/(c)).

**The yields exact-bar gate is intentionally different.** The execution-model
crate's own `asset_price` helper (`crates/execution-models/src/yields.rs`)
still uses the exact-ts bar: a non-stable yield deposit/withdraw is **rejected
without an exact bar** on its chain. That gate is load-bearing for #115 — it
converts *actual money* (gas to asset units, position `value_usd`), not a
sizing estimate, and a stale price for a real conversion is a different risk
than a stale price for sizing. The trait default `mark_close` (= exact bar
close) preserves this distinction; only the engine's `TickContext` overrides
it with the carry-forward, and only the engine's *sizing* path reads it.

### Absolute vs `"all"` vs relative

- **Absolute** (`"100"`): a literal quantity. For a swap buy it is quote/USD
  notional, for a swap sell base units; for a perp it is USD notional; for yield
  it is asset units. Resolved by `resolve_amount`'s `Absolute` arm as a no-op
  passthrough (`engine.rs:918`).
- **`"all"`**: the full-balance sentinel — since #124, the full **available**
  balance (`available.max(0)`, net of resting-order reservations). For an
  inline (same-bar) market swap the execution model interprets it at fill time
  (`swap.rs`'s `resolve_amount`); for any **queued** order — a resting limit or
  a deferred `next_open` market order (#116) — the engine **freezes `"all"` to
  an absolute amount at the decision bar** (the reservation needs an exact
  figure, and "what I held when I decided" is the only amount the decision
  could have meant). Yield deposit:
  `yields.rs` deposits the available balance **minus reserved gas**
  (`(available.max(0) - gas).max(0)`). Yield withdraw withdraws the
  whole position value. **There is no `"all"` handling in the perp model** —
  `perp.rs` always `parse()`s `size_usd`,
  and `parse` returns 0 on a non-numeric string,
  so `"all"` would size to 0. Use a `pct_*` basis or an absolute notional for
  perps.
- **Relative**: `{ basis, value }`, resolved to absolute as above.

## Which basis / when to use

- **`pct_balance`** — "trade X% of what I hold of this asset." The intuitive
  choice for *selling down* a spot holding (sell 50% of ETH) or depositing a
  fraction of cash into yield.
- **`pct_position`** — "trade X% of my open position." For perps this is the
  natural way to *scale out* (a `reduce_only` short sized to 50% of the long).
  For yield it is a fraction of principal+accrued.
- **`pct_portfolio`** — "risk X% of total equity," the conventional risk-sizing
  basis that accounts for everything you hold (spot + perp margin/PnL + yield),
  not just one asset. Use it when allocation should track NAV.

## Correctness notes / edge cases

- **Balance bases read available, not raw balance (#124, fixed).** Cash
  earmarked by a resting limit or a deferred `next_open` order is excluded
  from every `pct_balance` base and from the `"all"` sentinel
  (`available = balance − reserved`, clamped at zero), so a second action can
  neither size against nor spend funds an open order has committed. Equity
  (`pct_portfolio`'s base) still counts reserved cash — it is owned, just
  committed (see [portfolio-valuation](portfolio-valuation.md)). Pinned by
  `pct_balance_sizes_against_available_not_raw_balance`
  (`tests/issue_124_resting_reservation.rs`).

- **`pct_portfolio` uses tick-start equity — DECIDED SEMANTICS (#119(e)).**
  `tick_equity` is computed **once** at the top of each tick before any action
  runs and that single value is threaded into resting-order fills, initial
  actions, and signal-driven actions for the whole tick. **Every same-tick
  action sizes off tick-start equity; same-tick `pct_portfolio` actions do not
  compound against each other.** This is the intended behavior, not a tracked
  limitation:
  - Under strict/`next_open` profiles it is a **no-op**: deferred market
    orders don't touch the ledger on the decision tick, so there is no fresher
    equity to read anyway.
  - Under same-bar (research) profiles the residual effect is **fee-sized**
    (two 25% perps on $2000 size to $500 each instead of $500 then ~$499.875),
    never direction-changing.
  - Recomputing equity between same-tick actions would make sizing depend on
    **intra-tick action order** — exactly the dimension the
    rejected-as-unimplemented `ordering.same_tick` variants (#141) are supposed
    to govern. If a recompute is ever wanted, it should arrive as an explicit,
    wired sizing knob alongside #141, not as a silent default change.

  Snapshots recompute equity *after* the tick, so the snapshot semantics are
  unaffected. Pinned by `issue_119_same_tick_stale_tick_equity`
  (`tests/issue_119_price_lookups.rs`).

- **`pct_position` on a perp uses entry price, not mark.** The basis is
  `(p.size * p.entry_price).abs()` (`engine.rs:854`), the notional *at entry*, not
  `p.size * mark`. So "reduce 50% of position" means 50% of the entered notional
  regardless of how far mark has moved — the intended fraction of size stays
  stable, decoupled from unrealized PnL. (Closing logic in the perp model also
  divides requested USD by `entry_price` to get base units — `perp.rs:175`ish —
  so a `reduce_only` `size_usd` matching the opened notional closes the whole
  position, clamped to the open size at `perp.rs:176`.)

- **`pct_position` on a swap is rejected (#121 — FIXED).** A swap has no
  distinct position, and the old behavior — silently aliasing `pct_position`
  to `pct_balance` — masked configuration mistakes. `execute_action` now
  rejects it with `"pct_position is not valid for a swap"` before resolving
  the order (`engine.rs:991-996`), for market and limit swaps alike. The
  rejection surfaces as an `action_rejected` event each time the signal fires;
  promoting it to graph-validation time is a candidate follow-up.

- **`pct_portfolio` requires a price; rejects otherwise.** If `unit_price` is
  zero — a non-stable asset whose venue has **no mark** (no candle yet, or the
  carry-forward expired under `data.max_mark_staleness`) — `resolve_amount`
  returns the error `"pct_portfolio sizing needs a price for the action asset"`
  and the action is rejected, rather than dividing by zero or silently sizing
  to infinity. Since #119(d) this fires exactly when equity valuation also
  can't price the asset (same lookup), never on a mere intra-series gap.
  Perps are immune (their `unit_price` is the constant `Decimal::ONE`).

- **No look-ahead.** Equity, balances, and position notionals are all read from
  the ledger and the current/past bars: both `mark_price` (equity) and
  `mark_close` (sizing) use the venue-scoped close at-or-before `ts`
  (`close_at`), never a future bar. `pct_portfolio` uses tick-start equity,
  which is strictly start-of-tick state.

- **Money conservation / float safety.** Resolution is exact `Decimal` arithmetic
  except the final `.normalize().to_string()` (`engine.rs:933`); the resolved
  absolute amount then flows through the normal execution path, which enforces its
  own conservation invariants (a swap sell is rejected when fee+gas exceed
  proceeds — `swap.rs:178-182`, #117; a perp's settlement is floored at zero so a
  loss can't exceed the posted margin — `perp.rs:192`, #117). Sizing itself does
  not move money; it only computes the quantity the execution model then
  validates.

- **Zero / empty bases resolve to zero, not an error** (except `pct_portfolio`'s
  price guard). `pct_balance`/`pct_position` against a zero balance or a missing
  position yield `0` (the `unwrap_or(Decimal::ZERO)` defaults at `engine.rs:855`
  for perps, `engine.rs:954` for yield); the resulting zero-size action is then
  handled (typically rejected) downstream by the execution model, not by
  `resolve_amount`.

- **Malformed `value` is rejected, never sized to zero (#160, fixed).** The graph
  compiler validates every relative sizing value at compile time: it must parse
  as a decimal and be strictly positive, with errors naming the node. As a
  runtime backstop, `resolve_amount` returns an error (surfacing as
  `action_rejected`) for any malformed value that slips through, instead of the
  old `unwrap_or(Decimal::ZERO)` that silently sized the order to nothing.

### Known limitations

- **Perp `"all"` is rejected at compile time (#160, fixed)** — it used to parse
  to 0 and surface as a confusing runtime rejection. Only swaps and yields
  honor `"all"`.
- **Same-tick `pct_portfolio` actions do not compound** (#119(e), decided —
  see above): plan multi-action ticks as slices of the same tick-start equity.
- `pct_position` for a swap is **rejected** at fire time (#121, fixed) — use
  `pct_balance` or `pct_portfolio`.

## Tests

`crates/simulation-engine/tests/sizing.rs`:

- `swap_pct_balance_sells_half_the_from_asset` — `pct_balance` 50% of a 1.0 ETH
  holding leaves ~0.5 ETH.
- `swap_absolute_buy_spends_quote_notional` — an absolute `"100"` buy reports
  `value_usd == "100"` and `amount * fill_price ≈ 100`.
- `perp_pct_position_reduces_the_open_position` — open a 1000 USD long, then a
  `pct_position` 50% `reduce_only` short; both execute, none rejected (exercises
  the entry-notional position basis).
- `yield_pct_balance_deposits_a_fraction` — `pct_balance` 50% yield deposit
  executes, none rejected.
- `pct_portfolio_sizes_against_total_equity` — 10% of $1000 equity spent on ETH
  leaves ~$900 USDC (asserted in `[899, 900)`) — the USD-slice → from-asset-units
  conversion via `unit_price`.
- `pct_portfolio_perp_sizes_in_usd` — a `pct_portfolio` 25% perp on $2000 equity
  executes (one fill, no rejection); `size_usd = 25% * 2000 = 500` USD notional
  with no unit conversion (`unit_price = 1`).

`crates/simulation-engine/tests/issue_119_price_lookups.rs` pins the unified
unit-price semantics (#119(d)) and the same-tick snapshot decision (#119(e)):

- `issue_119_gap_sizing_resolves_fill_still_rejects_same_bar` — on a gap bar a
  `pct_portfolio` sell sizes at the carried mark; the same-bar fill still
  rejects on the exact-bar guard (`"no price for ETH on venueA"`).
- `issue_119_gap_sizing_defers_and_fills_next_open` — under strict_v1
  (`next_open`) the gap-bar signal sizes at the carried mark, defers, and
  fills at the next bar's open.
- `issue_119_sizing_respects_staleness_bound` — an expired mark
  (`data.max_mark_staleness`) rejects sizing through the zero-price guard, at
  the same tick the holding drops out of equity.
- `issue_119_same_tick_stale_tick_equity` — SPEC pin of the (e) decision: two
  same-tick 25% `pct_portfolio` perps both size off the shared tick-start
  equity (no intra-tick compounding).

The `"all"` sentinel paths are covered in the execution-model crate's own tests
(`crates/execution-models/tests/`), not in `sizing.rs`.

## Related issues

- [#117](https://github.com/ftvision/catalyst-backtest/issues/117) — margin cap — FIXED
- [#119](https://github.com/ftvision/catalyst-backtest/issues/119) — price lookups: sizing's unit price unified onto the bounded venue-scoped mark (d, FIXED); same-tick tick-start equity snapshot (e, DECIDED semantics) — RESOLVED
- [#121](https://github.com/ftvision/catalyst-backtest/issues/121) — pct_position semantics (perp entry-price basis intended; swap rejection FIXED)
- [#124](https://github.com/ftvision/catalyst-backtest/issues/124) — balance bases and `"all"` read available (balance − reserved); `"all"` frozen at the decision bar for queued orders — FIXED
- [#160](https://github.com/ftvision/catalyst-backtest/issues/160) — strict parsing of relative sizing values; perp `"all"` rejected — FIXED
