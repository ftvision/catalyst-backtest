# Portfolio valuation (equity / mark-to-market)

**Equity** is the single USD number that summarizes the portfolio at a tick — the
mark-to-market sum of every cash balance, perp position, and yield position. It's
what the trace reports as `equity_usd`, what the equity curve and every
performance metric are built from, and what `pct_portfolio` sizing resolves
against. Getting it right is a correctness concern: a mispriced leg silently
inflates or deflates every downstream number.

Computed by `compute_equity` in `crates/simulation-engine/src/engine.rs:1087`,
which walks the ledger's `to_portfolio()` snapshot
(`crates/portfolio-ledger/src/lib.rs:306`) and marks each leg.

## What it is

Equity is a sum over three leg types. Each is valued independently and added
into a running `Decimal`:

| Leg | Valuation rule | Code |
| --- | --- | --- |
| Cash balance, **stable** (USDC/USDT/USD/DAI/USDC.E) | `amount × 1` (1:1 USD) | `engine.rs:1094-1095` |
| Cash balance, **non-stable** (ETH, BTC, …) | `amount × mark_price` | `engine.rs:1096-1097` |
| **Perp** position | `margin_usd + unrealized_pnl(mark)` | `engine.rs:1101-1106` |
| **Yield** position | `principal + accrued` (the position's `value()`) | `engine.rs:1108-1110` |

**Stable detection** is `is_stable` (`crates/execution-models/src/pricing.rs:15`):
a case-insensitive match against `USDC | USDT | USD | DAI | USDC.E`. Anything
else is a non-stable and must be priced.

**The mark price** for non-stables and perps is `mark_price`
(`engine.rs:966`):

```
index.bar_at(venue, symbol, ts).map(|b| b.close)   // exact close on this venue/bar
    .or_else(|| index.price_any(symbol, ts))         // else any-venue fallback
```

i.e. **mark-to-close** of the bar at `ts` on the position's own venue, falling
back to `price_any` (`crates/simulation-engine/src/market.rs:154`): the symbol's
price on *any* venue, exact at `ts` or else the **last known value ≤ ts**
(`market.rs:156`).

**Perp PnL** is `PerpPosition::unrealized_pnl`
(`crates/portfolio-ledger/src/position.rs:48`):
`(mark − entry) × size` for a long, `(entry − mark) × size` for a short. So a
perp is valued at the margin you posted plus (or minus) the open PnL at the
current mark — never the full notional.

**Yield value** is `YieldPosition::value`
(`crates/portfolio-ledger/src/position.rs:89`): `principal + accrued`, the
redeemable amount, with no separate price lookup.

### When equity is snapshotted in the tick

Order matters, because accruals and fills mutate the ledger before it is valued.
Within each tick (`engine.rs:238-298`), in order:

1. `accrue_funding`, `accrue_yield`, `check_liquidations` mutate the ledger
   (`engine.rs:244-246`).
2. **`tick_equity = compute_equity(...)`** (`engine.rs:250`) — *tick-start* equity,
   computed once after accruals/liquidations but **before** this tick's
   resting-order fills and new actions. This is the value `pct_portfolio` sizing
   resolves against for every action this tick (`resolve_amount`,
   `engine.rs:910`), so all sizing within a tick uses one consistent equity.
3. Resting orders fill; initial + signal-driven actions run (more ledger mutation).
4. **`equity = compute_equity(...)`** again (`engine.rs:292`) — the *post-action*
   equity written into the `Snapshot` as `equity_usd` alongside the full
   `portfolio` (`engine.rs:293-297`).

So the snapshot's `equity_usd` reflects the **end-of-tick** state, while sizing
within the tick uses the **start-of-tick** value.

## Correctness notes / edge cases

- **No look-ahead in valuation.** Every leg is marked at the bar dated `ts` (the
  current tick) or, via `price_any`, the last known price **≤ ts**
  (`market.rs:156`). Valuation never reads a future bar. (This is distinct from
  fill pricing, which can use `next_open`; mark-to-market is close-of-current-bar.)

- **Stables are hard-coded to 1:1**, never priced (`engine.rs:1094`). A
  depeg is not modeled — a USDC balance is always worth its face in USD.

- **Perp loss is bounded at the posted margin in two independent places.**
  (a) At *valuation*: `margin_usd + unrealized_pnl(mark)` can go negative as a raw
  number, but a position that deep underwater is removed first by
  `check_liquidations` (`engine.rs:1067`), which fires once
  `unrealized_pnl(mark) ≤ −margin_usd` and settles nothing back
  (`engine.rs:1069`). (b) At *close*: settlement is floored at zero —
  `(returned_margin + realized_pnl − fee).max(0)`
  (`crates/execution-models/src/perp.rs:192`, #117) — so a leveraged loss can
  never claw back more than the margin posted. A still-open, not-yet-liquidated
  perp between ticks *can* contribute a value below `margin_usd` (down to ~0 at
  the liquidation boundary), which is correct mark-to-market.

- **Accruals feed valuation over actual elapsed time.** Both funding
  (`accrue_funding`, `engine.rs:970`) and yield (`accrue_yield`,
  `engine.rs:1019`) scale by `elapsed_secs = ts − prev_ts`
  (`engine.rs:241`), not a fixed interval (#118). Yield interest is
  `principal × apr × (elapsed / YEAR_SECONDS)` (`engine.rs:1028,1032`) — note it
  multiplies **`principal`**, not `principal + accrued`, so yield is **simple,
  not compounding**. The accrued amount lands in the position before the
  post-action `compute_equity`, so equity reflects it the same tick.

- **Determinism.** Equity is a pure function of the ledger snapshot and the
  indexed market data at `ts`. Balances and positions iterate in `BTreeMap`
  order (`portfolio-ledger/src/lib.rs:33-35,308`), and all arithmetic is
  `rust_decimal::Decimal`, so the sum is order-stable and exact (no float
  accumulation) — though the non-stable/perp legs are only as deterministic as
  the prices `mark_price` returns, which is the same indexed data the rest of the
  engine uses.

### Known limitations

- **Yield is valued 1:1 in USD regardless of the underlying asset (#115).**
  `compute_equity` adds `y.value()` — `principal + accrued`, raw **asset units** —
  straight into a USD total (`engine.rs:1109`), with no `mark_price` call.
  For a USDC/stable yield position this is correct. For a **non-stable** yield
  position (e.g. ETH staked in a pool), the principal+accrued *asset amount* is
  treated as if it were that many dollars — undervaluing or overvaluing the leg
  by the asset's price. Only the stable case is faithful today.

- **An unpriced non-stable balance is silently dropped (#119).** The cash-balance
  branch adds `amt × price` *only* `if let Some(price) = mark_price(...)`
  (`engine.rs:1096`); there is no `else`. If a non-stable asset has no bar on its
  venue and `price_any` finds nothing for the symbol, that holding contributes
  **0** to equity with no warning — equity understates the portfolio rather than
  erroring.

- **An unpriced perp drops its PnL but keeps its margin (#119).** When
  `mark_price` returns `None`, the perp contributes only `margin_usd`
  (`engine.rs:1104-1105`) — its unrealized PnL is silently treated as 0. The
  posted margin is preserved, but a winning or losing position with no current
  mark is valued as flat.

- **`price_any` fallback is venue-blind and can be stale (#119).** When the
  position's own venue has no bar at `ts`, `mark_price` falls back to
  `price_any`, which (1) ignores venue — it returns the symbol's price from *any*
  venue's series (`market.rs:154-156`, keyed only on `by_symbol`) — and
  (2) returns the **last known price ≤ ts** if there's no exact match
  (`market.rs:156`). So a perp on venue A can be marked at venue B's price, and a
  gappy series can mark a leg at a stale price carried forward from an earlier
  bar. Both are silent.

## Tests

`crates/portfolio-ledger/tests/ledger.rs` (the per-leg valuation primitives):
- `perp_unrealized_pnl_by_side` — `(mark−entry)·size` long vs `(entry−mark)·size`
  short, the exact PnL term equity adds for a perp (asserts +25 / −25 at mark 2100
  on a 0.25-size entry-2000 position).
- `accrue_then_withdraw_all_returns_principal_plus_interest` and
  `partial_withdraw_draws_accrued_first` — exercise `principal`/`accrued`, the two
  fields `YieldPosition::value()` (and thus equity) sums.
- `close_perp_credits_settlement_and_removes_position` — the ledger primitive
  `close_perp` credits the settlement it's handed (here 125 = 100 margin + 25 PnL)
  and removes the position; the close-time analog of the open-position valuation.
  (Note: the underwater settlement floor of #117 lives in `perp.rs:192`, the
  execute path, not in this ledger-level test.)
- `snapshot_reports_balances_positions_and_drops_zeros` — `to_portfolio()` drops
  zero balances, which is exactly what `compute_equity` iterates over.

`crates/simulation-engine/tests/sizing.rs` (equity feeding `pct_portfolio`,
i.e. `compute_equity` end-to-end through the tick):
- `pct_portfolio_sizes_against_total_equity` — 1000 USDC equity; a 10% slice
  spends ~100 USDC (asserts ~900 USDC remains), confirming stables are valued 1:1
  into equity.
- `pct_portfolio_perp_sizes_in_usd` — 2000 USDC equity; a 25% perp is intended to
  size to 500 USD notional. The test asserts the action **executes** (not rejected)
  off this equity rather than checking the exact notional, confirming
  USD-denominated perp sizing draws on the same equity figure.

Note: there is **no** dedicated unit test asserting the non-stable balance
mark-to-close path, the unpriced-leg drop (#119), the `price_any` venue-blind
fallback, or the non-stable yield 1:1 behavior (#115); those claims are grounded
in the `compute_equity`/`mark_price` source above, not in an executable test.

## Related issues

- [#115](https://github.com/ftvision/catalyst-backtest/issues/115) — non-stable yield valuation
- [#117](https://github.com/ftvision/catalyst-backtest/issues/117) — margin cap — FIXED
- [#118](https://github.com/ftvision/catalyst-backtest/issues/118) — elapsed accrual — FIXED
- [#119](https://github.com/ftvision/catalyst-backtest/issues/119) — inconsistent/stale/venue-blind price lookups; unpriced holdings dropped
