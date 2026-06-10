# Fill-price selection

**Fill-price selection** picks the *reference price* a market order executes at
from the bar's OHLC — before slippage and fees are applied on top. It is the
single most important **look-ahead** control in the backtester: the difference
between `next_open` and `close`/`open`/`mid`/`worse_side_ohlc` is the difference
between deciding on data you've already seen and *trading inside the same bar you
used to decide*. The policy field `fills.price_selection` selects the rule.

The policy is resolved once per run (`crates/simulation-engine/src/engine.rs:181`),
and the selection is then read on every fill in
`crates/execution-models/src/pricing.rs` — `reference_price`
(`pricing.rs:31`). The reference it returns is then fed to `apply_bps`
(the slippage haircut) and the fee/gas logic; this doc is only about which OHLC
value becomes the reference.

## What it is

`reference_price(bar, next, dir, policy)` matches on `policy.price_selection`
(`pricing.rs:37-47`):

| Variant | Reference price | Code | Look-ahead? |
| --- | --- | --- | --- |
| `next_open` | the **next** bar's `open` (the engine defers the fill to that bar; see timing note) | `pricing.rs:39` | **no** (next-bar) |
| `close` | this bar's `close` | `pricing.rs:38` | **yes** (same-bar) |
| `open` | this bar's `open` | `pricing.rs:40` | partial (see notes) |
| `mid` | `(bar.high + bar.low) / 2` | `pricing.rs:41` | **yes** (uses this bar's H/L) |
| `worse_side_ohlc` | `bar.high` on a buy, `bar.low` on a sell | `pricing.rs:42-45` | **yes** (same-bar, adverse) |

All five variants are **implemented** — the enum is
`PriceSelection { Close, Open, Mid, NextOpen, WorseSideOhlc }`
(`crates/simulation-policies/src/lib.rs:51`). `dir` is the trade
[`Direction`](../../crates/execution-models/src/pricing.rs) (`Buy`/`Sell`);
only `worse_side_ohlc` actually branches on it.

The `next` bar is supplied by the engine through `MarketContext::next_bar`
(`crates/execution-models/src/context.rs:36`), implemented for the live tick as
`TickContext::next_bar` → `BundleIndex::bar_after`
(`crates/simulation-engine/src/market.rs:203-205`, `137-143`), which returns
**the first bar strictly after the current tick's timestamp** for the
(venue, symbol). Both the swap path
(`crates/execution-models/src/swap.rs:92-93`) and the perp path
(`crates/execution-models/src/perp.rs:52-53`) fetch `next` and call
`reference_price` before applying slippage.

### Which profile uses which selection

| Profile | `price_selection` | Code |
| --- | --- | --- |
| `strict_v1` (default) | `next_open` | `crates/simulation-policies/src/profiles.rs:15` |
| `conservative_v1` | `worse_side_ohlc` | `profiles.rs:41` |
| `research_v1` | `close` | `profiles.rs:55` |

`conservative_v1` and `research_v1` inherit every other knob from `strict_v1`
via `..strict_v1()` and only override the selection (and other adversity knobs).
A per-run override can change `price_selection` on top of any profile **through
the policy contract's `fills.price_selection` field** (resolution:
`crates/simulation-policies/src/resolve.rs:64-65`; tested below).

> **Not** overridable via `config.execution`. The per-run `ExecutionOverrides`
> struct (`crates/contracts/src/request.rs:17-26`, applied at
> `engine.rs:183-186`) exposes only `signal_trigger`, `slippage_bps`,
> `gas_model`, and `action_cooldown` — it has **no** `price_selection` field. To
> change the selection per run you must set it in the submitted policy's
> `fills.price_selection`, which `resolve_policy` reads.

## Which to use / why choose one over another

- **`next_open` (strict default) — the honest default.** A signal/action decided
  on bar N's close fills at bar **N+1's open**, a price that did not exist when
  the decision was made. This is the only selection with **no intra-bar
  look-ahead**, and it is why `strict_v1` is the default profile
  (`profiles.rs:7-8`). Choose it for any result you intend to trust.
- **`worse_side_ohlc` (conservative) — pessimistic same-bar.** Buys fill at the
  bar **high**, sells at the bar **low** — the worst price the bar printed. This
  *does* peek inside the decision bar (you can't know the bar's high/low until it
  closes), but it errs **against** the trader, so it's a conservative stress
  rather than an optimistic cheat. Choose it for a deliberately punitive,
  user-facing backtest.
- **`close` (research) — fast but optimistic.** Fills at the same close you just
  evaluated the signal on. Convenient and the most common research convention —
  the same one backtesting.py exposes as `trade_on_close=True` and Backtrader as
  `cheat_on_close` — but it is **favorable same-bar look-ahead**: you transact at
  a price you already observed. Choose it only for quick exploration; never for a
  headline number. Every run under it carries a run-level warning saying exactly
  that (#122).
- **`open`** — fills at the *current* bar's open. The open is known at bar start,
  so it is less look-ahead-prone than `close`/`mid`, but the engine still
  evaluates and fills within the same tick, so it is not equivalent to the
  cross-bar `next_open` (see edge cases). Not used by any named profile; available
  by setting `fills.price_selection`.
- **`mid`** — `(high+low)/2`, a neutral mid that still requires the full bar's
  range, so it shares `close`'s same-bar look-ahead. Not used by any named
  profile.

## Correctness notes / edge cases

- **`next_open` is the central no-look-ahead guarantee.** The action is **decided**
  during tick N's processing (driven by tick N's data) but the order is **deferred**
  and fills during tick N+1's processing, at **tick N+1's open** — the engine fills
  it via `fill_pending_market` using a `price_selection = Open` clone, so
  `reference_price` returns the fill bar's own open. Either way the realized price is
  tick N+1's open, a price that did not exist at decision time. Demonstrated
  end-to-end by `strict_default_fills_at_next_bar_open_not_this_close`
  (`crates/simulation-engine/tests/no_look_ahead.rs`): a 500-USDC buy decided on bar
  0 (close 2000) fills at bar 1's open (2100), +10 bps → **2102.1**, not anything
  derived from 2000.
- **Deferral, not a price fallback, on the final bar (#116).** A market order under
  `next_open` is **deferred by the engine** and filled on the next bar at that bar's
  open (`fill_pending_market`, which fills via a `price_selection = Open` policy
  clone so `reference_price` returns the fill bar's open). When there is **no** next
  bar — a market order decided on the run's final bar — there is nothing to fill
  against, so the order **lapses unfilled** (recorded as `order_expired`); it does
  **not** fall back to the final close (that would be the same-bar look-ahead the
  deferral exists to prevent). Tested by `next_open_on_final_bar_does_not_fill`
  (`no_look_ahead.rs`): a single-bar run produces no `action_executed`, the ledger
  is untouched (full cash, no ETH), and the order is expired. (The
  `next.map(...).unwrap_or(bar.close)` fallback still exists inside `reference_price`
  for `NextOpen`, but the engine's deferral means a market order never reaches it on
  the final bar — the fallback is exercised only by the pricing unit tests, not the
  live market-order path.)
- **Booking time matches fill price under `next_open` (#116, fixed).** The fill is
  now both **filled and booked** at the fill bar: `fill_pending_market` stamps the
  `action_executed` event with the **fill bar's** `ts_iso`, and the position/cash
  effect lands on that bar. So a `next_open` fill is *booked* at bar N+1 and *priced*
  at bar N+1's open — decision time (bar N) and fill time (bar N+1) no longer
  conflate, and bar N's snapshot carries no phantom entry-bar P&L. Equity snapshots
  are still taken per tick at that tick's marks.
- **Same-bar fills are an explicit, warned convention (#122, decided).** The
  non-`next_open` selections read values (close, or the bar's high/low) that are
  only known once the bar has fully formed, yet the engine both *evaluates the
  signal* and *fills* within the same tick. `close` and `mid` bias **favorably**
  (you transact at a price you've already seen); `worse_side_ohlc` biases
  **adversely** (worst price of the bar). This is **kept on purpose** — it is the
  standard "trade-on-close" convention (backtesting.py `trade_on_close=True`,
  Backtrader `cheat_on_close`), and deferring a close-fill one bar would fill at
  bar *N+1*'s close, look-ahead in the other direction. The decision per #122 is
  to make the bias impossible to miss instead of removing the convention: `run()`
  pushes **one unconditional run-level warning** for every non-`next_open`
  selection (`engine.rs`, right where the run's `warnings` vector is created),
  with wording differentiated by bias direction (favorable for `close`/`mid`,
  stale-open for `open`, adverse stress for `worse_side_ohlc`). `next_open` runs
  stay warning-free.
- **Direction only matters for `worse_side_ohlc`.** The other four branches ignore
  `dir` (`pricing.rs:38-41`); `worse_side_ohlc` is the only one that maps
  buy→high / sell→low (`pricing.rs:42-45`), so its adversity is direction-aware.
- **Determinism.** The selection is a pure match over the resolved policy and the
  bar's OHLC — no clock, no RNG, no I/O. `mid` divides by the constant 2; given
  the same bars and policy the reference is bit-stable across runs.
- **Resting limit orders bypass selection.** Limit fills don't go through
  `reference_price`; they fill at the touched limit price via `execute_swap_at` /
  `execute_perp_at` (`engine.rs:766-767`) and are only eligible from the bar
  **after** placement (`engine.rs:733-736`), which is the limit-order analogue of
  the same no-look-ahead discipline. Price selection here only governs *market*
  orders.
- **Scope.** `reference_price` is the **reference** only; the adverse slippage
  haircut and fees are layered on afterward (swap: `swap.rs:100-102`; perp:
  `perp.rs:62`). See [slippage-models.md](slippage-models.md) for that layer. The
  `base_amount` passed to the volume slippage model is itself computed from the
  selected reference (`swap.rs:95-99`, `perp.rs:57-61`), so for a `next_open` buy
  the participation size is sized against next bar's open too.

## Tests (executable documentation)

`crates/simulation-engine/tests/no_look_ahead.rs`:
- `strict_default_fills_at_next_bar_open_not_this_close` — `next_open`
  fills at the *next* bar's open (2100 → 2102.1 after 10 bps), proving the decision
  bar's close (2000) is not used.
- `next_open_on_final_bar_does_not_fill` — a single-bar run has no next bar, so the
  deferred market order lapses unfilled (no `action_executed`, ledger untouched,
  `order_expired` recorded) — it does **not** fall back to the final close (#116).

`crates/simulation-engine/tests/issue_116_next_open_booking.rs` — the end-to-end
booking-time spec: an action decided on bar N is filled and booked on bar N+1
(`issue_116_spec_action_executes_at_next_tick_*`), bar N's snapshot is untouched
(`issue_116_entry_bar_*`), and a signal firing on the last bar does not execute
(`issue_116_spec_end_of_horizon_signal_action_not_executed_strict`).

`crates/simulation-engine/tests/issue_122_same_bar_convention.rs` — the decided
convention and its warning: a `research_v1` run carries the run-level look-ahead
warning while `strict_v1` carries none; a `strict_v1` run overridden to
`fills.price_selection = "close"` warns too (the warning follows the *executed*
selection); and the convention itself is SPEC-pinned — under `research_v1` with a
`level` trigger, a threshold signal computed from bar N's close (1700) fills on
bar N at 1700 + 5 bps = 1700.85, `action_executed` stamped at bar N's ts.

`crates/simulation-policies/tests/policies.rs`:
- `strict_profile_defaults` — asserts `price_selection == NextOpen` for
  `strict_v1` (`policies.rs:33`).
- `conservative_profile_is_more_adverse` — asserts `WorseSideOhlc` for
  `conservative_v1` (`policies.rs:42`).
- `resolved_policy_round_trips_through_json` — the enum serializes to the
  schema's snake_case `"next_open"` (`policies.rs:80`).
- `overrides_apply_on_top_of_profile` — sets `fills.price_selection =
  "worse_side_ohlc"` on the submitted policy contract and resolves it, flipping
  `strict_v1`'s `NextOpen` to `WorseSideOhlc` while untouched knobs keep their
  strict defaults (`policies.rs:97,105,108`). This is the policy-contract override
  path (`resolve.rs:64-65`), not `config.execution`.

The per-variant arithmetic of `open`/`mid`/`close`/`worse_side_ohlc` is not
directly asserted by a dedicated unit test in the files reviewed; the
`next_open` behavior (the look-ahead-critical case) is the one covered
end-to-end.

## Related issues

- [#116](https://github.com/ftvision/catalyst-backtest/issues/116) — next_open market orders deferred to fill+book on the fill bar (no phantom entry P&L) — **fixed**
- [#122](https://github.com/ftvision/catalyst-backtest/issues/122) — same-bar fills under close/open/mid/worse_side_ohlc selection — **decided convention** (trade-on-close, kept + per-run warning)
