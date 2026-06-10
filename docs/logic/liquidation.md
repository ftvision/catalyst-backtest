# Perp liquidation

**Liquidation** is the forced close of a leveraged perp when the mark price
crosses the position's **liquidation price** — the level at which its equity
(posted margin + unrealized PnL) falls to the policy's **maintenance margin**, a
configurable fraction of mark notional (#120). It's a correctness boundary on
losses: without it a backtest could let an underwater perp keep "losing" past
the collateral the trader ever put up, minting phantom debt and overstating
drawdown — and with only a *bankruptcy* trigger (the pre-#120 model) positions
survived later, and lost more, than a real venue would allow. The check runs
once per tick, near the top, before pending/resting-order fills and any action,
and is gated by the policy field `perps.liquidation_check`.

Implemented in `crates/simulation-engine/src/engine.rs` — `check_liquidations`,
called per tick from the main loop; the liquidation level itself is
`PerpPosition::liquidation_price` (`crates/portfolio-ledger/src/position.rs`).

## Trigger: the maintenance-margin model

For every open perp, each tick:

1. **Compute the liquidation price** from the position and the policy's
   `maintenance_margin_ratio` (`mmr`, a fraction of mark notional):

   ```text
   long:  p_liq = (entry·size − margin) / (size · (1 − mmr))
   short: p_liq = (entry·size + margin) / (size · (1 + mmr))
   ```

   This is the unique mark `p` where equity equals the maintenance requirement:
   `margin + unrealized_pnl(p) = mmr · size · p`. `size` is absolute base units
   (always > 0 for an open position) and validation guarantees `0 ≤ mmr < 1`,
   so the denominator is never zero. At `mmr = 0` the formula degenerates to
   the bankruptcy price (loss = full margin) — the exact pre-#120 trigger. A
   long with margin exceeding notional (sub-1x leverage) yields a negative
   level, i.e. "cannot be liquidated".

2. **Mark at the intrabar extreme** (#120 wick half, landed in #152): the bar's
   **low** for a long, **high** for a short — the worst price the position
   touches *within* the bar, not just the close. If no candle exists for
   `(venue, symbol)` this tick, fall back to the last-known venue-scoped mark
   (bounded by `data.max_mark_staleness`, #119(b)); if there is no mark at all
   the position is **skipped** this tick (never liquidated on absent data).

3. **Trigger** when the adverse extreme crosses the level:
   `low ≤ p_liq` (long) / `high ≥ p_liq` (short).

## Settlement: residual at the breach price

On trigger the position is closed via `ledger.close_perp` and the **residual
equity is settled back** to the venue's USDC:

```text
fill       = min(bar.open, p_liq)   (long)    /   max(bar.open, p_liq)   (short)
settlement = max(margin + unrealized_pnl(fill), 0)
```

- **No gap:** when the bar *opened* on the safe side of `p_liq` and only traded
  through it intrabar, the fill is `p_liq` itself and the settlement is exactly
  the maintenance requirement, `mmr · size · p_liq`.
- **Gap through the level:** when the bar *opened* beyond `p_liq` (a gap), the
  engine can't pretend it closed at a price that never traded — the fill is the
  bar's **open**, the earliest (and worse) price of the bar, and the residual
  shrinks accordingly.
- **Gap through bankruptcy:** if the open is beyond even the bankruptcy price,
  the residual is negative and **clamps at zero** — a liquidation can never
  claw back collateral the trader never posted. This is the same invariant the
  margin-cap fix (#117) enforces on the ordinary close path
  (`crates/execution-models/src/perp.rs`: `settlement = (returned_margin +
  realized_pnl - fee).max(Decimal::ZERO)`). The two paths stay consistent.
- On the no-bar fallback-mark path, the fill is the fallback mark itself.

A `"liquidation"` event is pushed with `reason`
`"{venue} {symbol} position liquidated"` and a `detail` carrying `venue`,
`symbol`, `mark` (the fill price), `liquidation_price` (`p_liq`), `settled_usd`
(the residual credited back), and `margin_lost_usd` (`margin − settled_usd`).

Every portfolio snapshot (per-tick and final) also reports each open perp's
`liquidation_price` — computed from the executed policy's ratio — in
`PerpPosition::to_contract` (previously a dead `None`).

### Policy knobs

| Knob | Values | Behavior |
| --- | --- | --- |
| `perps.liquidation_check` | `every_tick` (default) / `never` | Run the maintenance check every tick, or never force-close. |
| `perps.maintenance_margin_ratio` | decimal string in `[0, 1)`, default `"0.0125"` | Maintenance margin as a fraction of mark notional. `"0"` = liquidate only at full bankruptcy (the pre-#120 model). |

The default `0.0125` (1.25%) is **Hyperliquid's top-tier maintenance margin**:
half the initial margin at its 40x maximum leverage, `1/(2·40)`. All three
profiles (`strict_v1`, `conservative_v1`, `research_v1`) share it. The executed
ratio is echoed in the trace's policy block (#157), so every result
self-documents the liquidation model it ran under. Validation (#160 discipline)
rejects malformed ratios and any ratio ≥ 1 (a maintenance margin of 100% of
notional is nonsensical — it would liquidate every position at entry); a bad
value never silently means "no maintenance buffer".

### Explicit v1 assumptions

- **Flat ratio, not tiered.** Real venues scale maintenance margin with
  position notional (larger positions → higher tiers). v1 applies one flat
  ratio to every position; a tier table is a possible future extension of the
  same knob.
- **No liquidation penalty.** The position settles its full residual at the
  breach price; no liquidator fee or insurance-fund haircut is charged. Real
  venues typically take a cut, so v1 is slightly *favorable* at liquidation. A
  `liquidation_penalty` knob is the named future extension.
- **No partial liquidation.** The whole position closes at once.

## When to use which

- **`every_tick`** (default): realistic — a leveraged position that breaches
  its maintenance level mid-backtest is removed, its residual settled, exactly
  once, so later ticks don't keep marking a position a real venue would have
  closed.
- **`never`**: a research/idealized setting. Use it to study a strategy's raw
  signal PnL without venue liquidation mechanics, or when you've modeled risk
  some other way. Not realistic — an underwater position survives and its
  negative unrealized PnL keeps dragging equity without ever being capped by a
  forced close (the #117 floor still protects the eventual *close* settlement).

## Correctness notes / edge cases

- **No look-ahead.** The trigger reads the current tick's bar (its low/high/open
  are known once the bar closed — the same convention as every other intrabar
  mechanism, e.g. resting-limit fills) and runs near the *top* of the tick,
  after funding/yield accrual but before order fills and actions, so it only
  ever uses already-settled state.

- **Order-of-events nuance.** A position opened by a deferred `next_open` fill
  books *after* the liquidation pass of its fill bar, so the first bar that can
  liquidate it is the next one. A wick on the very fill bar is therefore not
  retroactively applied — consistent with the engine's no-intra-tick-reordering
  rule.

- **Funding can trigger liquidation the same tick (#165).** Under strict
  balance policy a funding charge larger than free cash deducts the shortfall
  from the position's posted margin (see
  [funding-accrual](funding-accrual.md)). That happens in `accrue_funding`,
  which runs *before* this check in the same tick — so a margin reduction that
  breaches maintenance is caught immediately at the tightened `p_liq`, settling
  whatever residual margin the cascade left.

- **Degenerate pin.** `maintenance_margin_ratio = "0"` reproduces the exact
  pre-#120 behavior: trigger at `unrealized_pnl ≤ −margin`, settlement clamped
  to zero at any gap. Pinned by
  `issue_120_zero_ratio_degenerates_to_old_bankruptcy_trigger`.

- **Money conservation / loss cap.** `settlement = max(margin + pnl(fill), 0)`
  is never negative and never exceeds what equity actually was at the fill: a
  leveraged loss can never exceed the posted margin (#117 invariant), whether
  the position is force-liquidated or closed by the strategy underwater.

- **Determinism.** `check_liquidations` iterates a snapshot
  `Vec<PerpPosition>` collected up front and uses pure `Decimal` arithmetic
  against deterministic bar data; no randomness, no floats in the trigger. Same
  inputs ⇒ same liquidations, same fills, same events.

- **Missing-mark skip.** If neither a bar nor a (staleness-bounded) carried
  mark exists for the position's venue at `ts`, the position is skipped that
  tick — it is *not* liquidated on absent data, and is re-evaluated next tick.

### Known limitations (summary)

- Flat maintenance ratio — not tiered by notional (v1 assumption).
- No liquidation penalty / insurance-fund haircut (v1 assumption; future knob).
- No partial liquidation.
- Binary `every_tick`/`never` cadence only.

## Tests

- `crates/simulation-engine/tests/issue_120_liquidation_realism.rs` —
  end-to-end engine coverage: wick liquidation both sides; no over-liquidation
  above `p_liq`; the flipped maintenance test (`…liquidated_at_maintenance_
  level_before_full_bankruptcy`: mark 1802 sits between bankruptcy 1801.8 and
  `p_liq ≈ 1824.61`, liquidates with residual ≈ 0.0999); exact no-gap residual
  `mmr·size·p_liq` (long and short); gap-through-bankruptcy clamps to zero;
  `mmr = "0"` degenerate pin (both sides of the old boundary);
  `liquidation_price` populated in the final portfolio snapshot.
- `crates/portfolio-ledger/tests/ledger.rs` — `perp_liquidation_price_by_side`
  (formula per side, mmr-0 degeneration, residual identity);
  `perp_unrealized_pnl_by_side`; snapshot reports `liquidation_price`.
- `crates/simulation-policies/tests/policies.rs` — default `0.0125` in every
  profile; contract override; `"0"` accepted; malformed / negative / ≥ 1
  rejected with exact messages; executed-ratio echo in `to_contract`.
- `crates/execution-models/tests/execution.rs` —
  `leveraged_long_loss_is_capped_at_posted_margin`: the close-path analogue of
  the settlement floor (#117).
- `crates/result-reporter/tests/reporter.rs` — `liquidation_is_logged`: the
  `"liquidation"` event surfaces as a `liquidation` trade row.

## Related issues

- [#117](https://github.com/ftvision/catalyst-backtest/issues/117) — leverage loss capped at margin — FIXED
- [#120](https://github.com/ftvision/catalyst-backtest/issues/120) — intrabar wick marking + maintenance-margin liquidation — FIXED
