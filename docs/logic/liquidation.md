# Perp liquidation

**Liquidation** is the forced close of a leveraged perp when the position has lost
its entire posted margin. It's a correctness boundary on losses: without it a
backtest could let an underwater perp keep "losing" past the collateral the
trader ever put up, minting phantom debt and overstating drawdown. The check runs
once per tick, near the top, before resting-order fills and any action, and is
gated by the policy field `perps.liquidation_check`.

Implemented in `crates/simulation-engine/src/engine.rs` — `check_liquidations`
(`engine.rs:1053-1084`), called per tick at `engine.rs:246`.

## What it is

For every open perp, each tick:

1. Compute the **mark price** — `mark_price` (`engine.rs:966-968`): the bar `close`
   for `(venue, symbol)` at `ts`, falling back to `price_any(symbol, ts)`. If no
   mark is available the position is **skipped** (`engine.rs:1066`,
   `else { continue }`).
2. Trigger if **unrealized PnL ≤ −margin**:
   ```
   p.unrealized_pnl(mark) <= -p.margin_usd        // engine.rs:1067
   ```
   `unrealized_pnl` (`crates/portfolio-ledger/src/position.rs:48-53`) is
   `(mark − entry)·size` for a long, `(entry − mark)·size` for a short.
3. On trigger: **close the position, settle nothing**:
   ```
   ledger.close_perp(&p.venue, &p.symbol, Decimal::ZERO)   // engine.rs:1069
   ```
   `close_perp` (`crates/portfolio-ledger/src/lib.rs:167-182`) removes the
   position and credits the settlement back to the venue's USDC — here the
   settlement is `Decimal::ZERO`, so the full posted `margin_usd` is lost and
   nothing is returned. A `"liquidation"` event is pushed
   (`engine.rs:1070-1081`) with `reason` `"{venue} {symbol} position liquidated"`
   and a `detail` carrying `venue`, `symbol`, `mark`, and `margin_lost_usd`.

The trigger is **full bankruptcy** — loss equal to or beyond the *entire* margin.
There is no partial liquidation and no maintenance-margin buffer (see below).

### Policy options (`perps.liquidation_check`)

| Value | Behavior | Status |
| --- | --- | --- |
| `every_tick` | Run the bankruptcy check every tick (the default in all profiles). | implemented |
| `never` | Skip the check entirely — positions are never force-closed. | implemented |

The enum is `LiquidationCheck { EveryTick, Never }`
(`crates/simulation-policies/src/lib.rs:96`). It is **two-valued** — there is no
"every N ticks", "on-close-only vs intrabar", or maintenance-margin variant. All
shipped profiles default to `EveryTick` via the shared base builder
(`crates/simulation-policies/src/profiles.rs:30`); it can be overridden from
policy input (`crates/simulation-policies/src/resolve.rs:125-126`).

## When to use which

- **`every_tick`** (default): realistic — a leveraged position that goes bankrupt
  mid-backtest is removed and its margin written off, so later ticks don't keep
  marking a position that would have been wiped out on a real venue.
- **`never`**: a research/idealized setting. Use it to study a strategy's raw
  signal PnL without venue liquidation mechanics, or when you've modeled risk
  some other way. Not realistic — an underwater position survives and its negative
  unrealized PnL keeps dragging equity (`compute_equity`, `engine.rs:1101-1107`,
  adds `margin_usd + unrealized_pnl(mark)` per perp when a mark exists, else just
  `margin_usd`) without ever being capped by a forced close.

## Correctness notes / edge cases

- **No look-ahead.** The trigger reads the **bar close** at the current tick `ts`
  (`mark_price`, `engine.rs:966-968`) — known, past-or-present data, not a future
  bar. The check also runs near the *top* of the tick (`engine.rs:246`), after
  funding/yield accrual but before resting-order fills and any action this tick,
  so it only ever uses already-settled state.

- **Marks at bar CLOSE only — ignores the intrabar wick (#120).** Liquidation
  fires only if the *close* breaches `−margin`. A bar whose **low/high wick** blew
  through the bankruptcy price but whose close recovered will **not** liquidate —
  real venues mark continuously and would have liquidated on the wick. This
  understates liquidation frequency and is a known limitation (#120).

- **No maintenance-margin buffer (#120).** Real venues liquidate at a
  *maintenance margin* — before equity hits zero (e.g. at ~0.5–5% remaining). Here
  the threshold is the full margin (`unrealized_pnl <= -margin_usd`), i.e.
  liquidation only at 100% loss of collateral. Backtests therefore liquidate
  *later* (and less often) than a real venue (#120).

- **Money conservation / loss cap.** Settlement on liquidation is hard-zero
  (`engine.rs:1069`): the position's margin is fully lost and nothing negative is
  ever credited, so a liquidation cannot claw back collateral the trader never
  posted. This is the same invariant the **margin-cap fix (#117)** enforces on the
  ordinary close path: a normal `close_perp` floors settlement at zero —
  `let settlement = (returned_margin + realized_pnl - fee).max(Decimal::ZERO);`
  (`crates/execution-models/src/perp.rs:192`). The two paths are consistent: a
  leveraged loss can never exceed the posted margin, whether the position is
  force-liquidated (settles 0) or the strategy closes it underwater (settles
  `max(…, 0)`). A bankrupt position that the engine liquidates and one the strategy
  closes at the same underwater mark both end at "margin gone, nothing returned".

- **Determinism.** `check_liquidations` iterates a snapshot
  `Vec<PerpPosition>` collected up front (`engine.rs:1064`) and uses pure
  `Decimal` arithmetic against the deterministic mark; no randomness, no floats in
  the trigger. Same inputs ⇒ same liquidations and same events.

- **Missing-mark skip.** If neither a bar nor any cross-venue price exists for the
  symbol at `ts`, the position is skipped that tick (`engine.rs:1066`) — it is
  *not* liquidated on absent data, and is re-evaluated next tick once a mark
  exists.

- **`never` does not cap losses.** Under `never` the bankruptcy check is bypassed
  (`engine.rs:1061-1063`); an underwater position persists and continues to mark
  at `margin_usd + unrealized_pnl` in equity. The `#117` floor still protects the
  eventual *close* settlement, but equity in the interim can show the position's
  full negative unrealized PnL.

### Known limitations (summary)

- Close-only marking; intrabar wick ignored (#120).
- No maintenance margin — triggers at full bankruptcy, not earlier (#120).
- Binary `every_tick`/`never` only — no partial liquidation, no liquidation price
  reported (`to_contract` sets `liquidation_price: None`,
  `crates/portfolio-ledger/src/position.rs:64`).

## Tests

The trigger condition, the settlement-zero semantics, and the margin-cap
invariant are each covered, but **note**: there is no engine-level integration
test that actually fires `check_liquidations` end-to-end (no test under
`crates/simulation-engine/tests/` exercises the bankruptcy path). The behavior is
established by the component-level tests below.

- `crates/portfolio-ledger/tests/ledger.rs` —
  `perp_unrealized_pnl_by_side` (`ledger.rs:124`): confirms the
  `unrealized_pnl` signing per side that the trigger
  (`unrealized_pnl(mark) <= -margin_usd`) depends on — `+25` for a long, `−25`
  for a short on a `+100` mark move.
- `crates/execution-models/tests/execution.rs` —
  `leveraged_long_loss_is_capped_at_posted_margin` (`execution.rs:202`): a 10x
  long crashed ~15% (~150% of margin) closes with USDC unchanged at `899.5` — the
  loss caps at the posted margin (the `#117` floor), never clawing back unposted
  collateral. This is the close-path analogue of liquidation's settle-nothing.
- `crates/result-reporter/tests/reporter.rs` —
  `liquidation_is_logged` (`reporter.rs:122`): a `"liquidation"` event is surfaced
  as a `liquidation` trade row with the symbol present and a non-empty reason (the
  reporter side of the event emitted at `engine.rs:1070-1081`).
