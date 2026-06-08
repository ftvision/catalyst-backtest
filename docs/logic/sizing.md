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
| `pct_balance` | the relevant asset balance (swap from-asset, yield asset, perp cash/USDC) | swap, yield, perp |
| `pct_position` | the relevant open position notional/principal | perp, yield (perp swap aliases balance — see below) |
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
subtype in `execute_action` (`engine.rs:812-895`):

- **swap** (`engine.rs:820-848`): `balance = position = ledger.balance(chain, from_asset)`
  — a swap has no distinct "position", so **both** `pct_balance` and
  `pct_position` resolve against the from-asset balance (they alias;
  `engine.rs:825-827` passes `bal` twice). `unit_price` is the from-asset mark
  (`asset_price`, 1 for stables) so `pct_portfolio` converts the USD slice back
  into from-asset units.
- **perp** (`engine.rs:849-878`): `balance = ledger.balance(chain, "USDC")`;
  `position = (p.size * p.entry_price).abs()` — the open position's **entry**
  notional (`engine.rs:854`); `unit_price = Decimal::ONE` because `size_usd` is
  already USD (so `pct_portfolio` needs no conversion).
- **yield** (`resolve_yield_amount`, `engine.rs:938-958`): `balance =
  ledger.balance(chain, asset)`; `position = principal + accrued` of the matching
  yield position (`engine.rs:953`); `unit_price` is the asset mark (1 for
  stables).

`unit_price` for non-perp assets comes from `asset_price` (`engine.rs:897-904`):
1 for stables, else the bar close, else 0.

### Absolute vs `"all"` vs relative

- **Absolute** (`"100"`): a literal quantity. For a swap buy it is quote/USD
  notional, for a swap sell base units; for a perp it is USD notional; for yield
  it is asset units. Resolved by `resolve_amount`'s `Absolute` arm as a no-op
  passthrough (`engine.rs:918`).
- **`"all"`**: the full-balance sentinel. It is **not** rewritten by the engine's
  `resolve_amount` (it is an `Absolute`, so it passes through unchanged); the
  execution models interpret it at fill time. Swap: `swap.rs:44-50`
  (`resolve_amount`) spends the entire from-asset balance. Yield deposit:
  `yields.rs:32-37` deposits the asset balance **minus reserved gas**
  (`(balance - gas).max(0)`). Yield withdraw: `yields.rs:74-78` withdraws the
  whole position value. **There is no `"all"` handling in the perp model** —
  `perp.rs` always `parse()`s `size_usd` (`perp.rs:93` on open, `perp.rs:175` on
  close), and `parse` (`pricing.rs:117-119`) returns 0 on a non-numeric string,
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

- **`pct_portfolio` uses tick-start equity (stale within a tick).** `tick_equity`
  is computed **once** at the top of each tick before any action runs
  (`engine.rs:248-250`, `compute_equity` at `engine.rs:1087-1112`) and that single
  value is threaded into resting-order fills (`engine.rs:253-256`), initial
  actions (`engine.rs:260-268`), and signal-driven actions (`engine.rs:270-290`)
  for the whole tick. If a signal fires **multiple** actions in one tick (or
  several signals fire), every `pct_portfolio` action sizes off the **same**
  start-of-tick equity — it does **not** see equity changes from earlier actions
  in the same tick. Snapshots recompute equity *after* the tick (`engine.rs:292`),
  so the staleness is intra-tick only. This is deterministic but can over-allocate
  if two actions each take "10% of portfolio" expecting compounding.

- **`pct_position` on a perp uses entry price, not mark.** The basis is
  `(p.size * p.entry_price).abs()` (`engine.rs:854`), the notional *at entry*, not
  `p.size * mark`. So "reduce 50% of position" means 50% of the entered notional
  regardless of how far mark has moved — the intended fraction of size stays
  stable, decoupled from unrealized PnL. (Closing logic in the perp model also
  divides requested USD by `entry_price` to get base units — `perp.rs:175`ish —
  so a `reduce_only` `size_usd` matching the opened notional closes the whole
  position, clamped to the open size at `perp.rs:176`.)

- **`pct_position` on a swap aliases `pct_balance`.** A swap has no distinct
  position, so `execute_action` passes the from-asset balance as **both** the
  balance and position bases (`engine.rs:825-827`). A `pct_position` swap
  therefore resolves identically to `pct_balance`. This is intentional aliasing,
  not position tracking; documented here as a known shape, not a bug.

- **`pct_portfolio` requires a price; rejects otherwise.** If `unit_price` is zero
  — e.g. a non-stable from-asset with no bar this tick (`asset_price` returns 0) —
  `resolve_amount` returns the error `"pct_portfolio sizing needs a price for the
  action asset"` (`engine.rs:925-929`) and the action is rejected (the swap branch
  propagates the `Err` at `engine.rs:828-829`), rather than dividing by zero or
  silently sizing to infinity. Perps are immune (their `unit_price` is the
  constant `Decimal::ONE`).

- **No look-ahead.** Equity, balances, and position notionals are all read from
  the ledger and the current/past bar (`mark_price` at `engine.rs:966-968` uses
  `bar_at(...,ts)` or a prior `price_any`, never a future bar). `pct_portfolio`
  uses tick-start equity, which is strictly start-of-tick state.

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

- **`value` parse failure defaults to 0.** `value.parse::<Decimal>().unwrap_or(Decimal::ZERO)`
  (`engine.rs:920`) — a malformed percentage sizes to zero rather than aborting.

### Known limitations

- **Perp `"all"` is unsupported** (parses to 0 — `perp.rs:93`/`perp.rs:175` via
  `parse`, `pricing.rs:117-119`). Only swaps and yields honor `"all"`.
- **Intra-tick `pct_portfolio` staleness** (above) — by design, but a caveat for
  multi-action ticks.
- `pct_position` for a swap is an **alias** for `pct_balance`, not real
  position-aware sizing.

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

The `"all"` sentinel paths are covered in the execution-model crate's own tests
(`crates/execution-models/tests/`), not in `sizing.rs`.

## Related issues

- [#117](https://github.com/ftvision/catalyst-backtest/issues/117) — margin cap — FIXED
- [#121](https://github.com/ftvision/catalyst-backtest/issues/121) — pct_position semantics (perp entry-price basis; swap aliases pct_balance)
