# Derived signal sources

A **derived source** is a rolling transform over another source's recent bar
values — a moving average of price, a breakout high/low, a rate of change. It
lets a signal compare a live value against its own recent history (price vs its
SMA, funding vs its EMA). For correctness it matters because (a) each transform
has a precise formula and window, (b) it must use only **past/known** bars — no
look-ahead — and (c) it must refuse to fire until it has a full **warmup**
window of history.

A derived source is `Source::Derived { of, transform, window }`
(`crates/contracts/src/graph.rs:226`). It nests: `of` is any other `Source`
(including another `Derived`), so you can stack transforms. It is read per tick
by `source_value` in the engine and reduced by `apply_transform`
(`crates/simulation-engine/src/engine.rs:486`, `:507`).

## What it is

Each tick, `source_value` for a `Derived` samples the underlying source `of` at
the last `window` grid bars, **newest first**, then applies the transform
(`crates/simulation-engine/src/engine.rs:486-502`):

```rust
let w = (*window).max(1) as i64;
let mut samples = Vec::with_capacity(w as usize);
for k in 0..w {
    match source_value(of, index, ts - k * interval_secs, interval_secs) {
        Some(v) => samples.push(v),
        None => break,            // stop at the first gap
    }
}
if (samples.len() as u32) < *window {  // full window required
    return None;
}
Some(apply_transform(transform, &samples))
```

So `samples[0]` is the **current** bar (`ts`), `samples[1]` is `ts - interval`,
… `samples[window-1]` is the oldest. The sampler stops at the first missing bar.

### Transforms

`Transform` is the enum at `crates/contracts/src/graph.rs:236-247`; reduced by
`apply_transform` at `crates/simulation-engine/src/engine.rs:507-534`. All
operate on the `samples` vector (newest-first, length == `window`).

| Variant | Formula | Code | Status |
| --- | --- | --- | --- |
| `sma` | `sum(samples) / samples.len()` — simple mean | `engine.rs:509-511` | implemented |
| `ema` | exponential MA, `alpha = 2 / (window + 1)`, folded **oldest→newest** seeded at the oldest sample | `engine.rs:523-532` | implemented |
| `rolling_high` | `max(samples)` over the window | `engine.rs:512` | implemented |
| `rolling_low` | `min(samples)` over the window | `engine.rs:513` | implemented |
| `roc` | `(current - oldest) / oldest`; returns `0` if `oldest == 0` | `engine.rs:514-522` | implemented |

Notes on the formulas:
- **`sma`** divides by `samples.len()` which, past the warmup gate, equals
  `window` (`engine.rs:510`).
- **`ema`** is a *windowed* EMA: it seeds at the oldest sample in the window and
  folds forward to the newest with `alpha = 2/(n+1)` where `n = samples.len()`
  (`engine.rs:525`). Past the warmup gate `n == window`. It is **not** a
  full-history EMA carried across the whole run — only the last `window` bars
  contribute, and there is no persisted EMA state between ticks (each tick
  recomputes from scratch).
- **`roc`** is the return vs the **oldest** sample in the window, i.e. over
  `window - 1` bar steps: `(samples[0] - samples[last]) / samples[last]`
  (`engine.rs:514-522`). Division-by-zero is guarded to `0`.
- **`rolling_high` / `rolling_low`** use `max`/`min` with an
  `unwrap_or(Decimal::ZERO)` fallback (`engine.rs:512-513`); past the warmup
  gate `samples` is always non-empty, so the `ZERO` default is unreachable in
  practice.

The underlying `of` can be any source — most often `price`, but also `funding`,
`yield`, `gas`, or a nested `derived` (`graph.rs:204-231`).

## Which / when to use

A derived source is the right-hand or left-hand side of a `threshold` signal
(`Reference::Source` / `ThresholdConfig.source`, `graph.rs:260,269`). Typical
uses:

- **`sma` / `ema`** — trend filters: fire when price crosses below/above its
  moving average. `ema` weights recent bars more, so it turns faster than `sma`
  of the same window.
- **`rolling_high` / `rolling_low`** — breakout / Donchian-style signals: price
  making a new N-bar high or low.
- **`roc`** — momentum: fire when the N-bar rate of change exceeds a threshold.

These are signal-shaping primitives; choose by what the strategy needs to
compare against (a smoothed level vs. an extreme vs. a return).

## Correctness notes / edge cases

- **No look-ahead.** Sampling runs `k = 0..window` over `ts - k*interval_secs`
  (`engine.rs:491-492`) — the current bar and **strictly earlier** bars. No
  future bar (`ts + …`) is ever read, so a derived value at tick *t* depends
  only on bars at or before *t*. The current bar's value is its `close`
  (`Source::Price` → `bar.close`, `engine.rs:477`), which is the known
  end-of-bar price. (Note: derived values use the current bar's *close*; this is
  the value the signal reads, not the fill price. Market fills go through the
  execution model separately — see slippage-models.md / no_look_ahead.rs — so
  reading the current bar's close in a signal is consistent rather than
  forward-looking.)
- **Warmup / insufficient history.** If fewer than `window` samples are
  collected, `source_value` returns `None` (`engine.rs:498-500`). A `None`
  source value makes the threshold un-evaluable, so the signal does **not** fire
  during warmup (`engine.rs:371-378` treats a missing leaf value as no
  condition this tick). This covers both run-start (not enough bars before the
  start) and the case where the configured `window` simply exceeds the available
  series length.
- **Gaps abort the window.** The sampler `break`s at the first bar that returns
  `None` (`engine.rs:494`). Because the count check then sees fewer than
  `window` samples, **any** missing bar inside the window suppresses the derived
  value for that tick (returns `None`) rather than averaging over a short or
  hole-punched window. (Note: the engine also has an independent interior-gap
  guard on required candle series, `check_required_coverage`,
  `engine.rs:135-165`; the derived sampler's own `break` is the local, per-read
  defense.)
- **`window = 0` guard.** `w = (*window).max(1)` prevents a zero-length loop
  (`engine.rs:487`), but the warmup check still compares against the raw
  `*window`; with `window = 0` the check `0 < 0` is false, so it would emit a
  one-sample transform. A meaningful window should be `>= 1`.
- **Determinism.** All arithmetic is on `Decimal` (fixed-point) with
  deterministic iteration over a fixed grid; `sma`, `roc`, `rolling_*` are exact.
  `ema` uses `Decimal` throughout as well (no float), so it is deterministic; the
  only inexactness is the fixed-point rounding of `2/(n+1)` and the repeated
  multiply-add, which is reproducible run-to-run.
- **`lookback_bars` in data requirements.** The compiler advertises how much
  pre-start history the data layer must fetch. `source_lookback` sums the
  `window`s along any **nested** derivation chain
  (`graph-compiler/src/lib.rs:731-737`):
  `Derived { of, window, .. } => window.saturating_add(source_lookback(of))`,
  base sources contribute `0`. `data_requirements` takes the `max` over a
  threshold's source and (if present) its reference source
  (`lib.rs:657,667`) and stores it as `DataRequirements.lookback_bars`
  (`lib.rs:139,681`), documented as "fetch `start - lookback_bars * interval`"
  (`lib.rs:136-137`). So a stacked `sma(window=5)` of an `sma(window=10)` reports
  `15` bars of warmup, matching the engine's nested sampling (the outer needs 5
  valid inner values, each of which needs 10 bars).
- **Known limitation — nested-window lookback is summed, not exact.**
  `source_lookback` *sums* nested windows, which is a safe upper bound for the
  worst case but slightly conservative (the outer window's bars overlap the inner
  sampling). This over-fetches rather than under-fetches, so it does not cause
  look-ahead or warmup failures. No tracking issue is recorded for this; flagged
  here as an observation, not a known bug.
- **No persisted state.** Because every tick recomputes from the raw window,
  there is no carried EMA/accumulator state that could drift or leak across
  ticks; correctness does not depend on tick ordering beyond the chronological
  grid.

## Tests

`crates/simulation-engine/tests/derived.rs`:
- `price_crossing_below_its_moving_average_fires_once` — `sma(3)` of price as the
  threshold reference; closes `100,100,100,90,100` with a `crossing` trigger and
  `<` operator fire exactly once (the bar where price dips below its 3-bar
  average), asserting `signal_fired == 1` and `action_executed == 1`
  (`derived.rs:90-102`). Confirms the SMA formula and the past-only window.
- `derived_signal_does_not_fire_before_warmup` — `sma(5)` (`<`, `level` policy)
  with only 3 bars of history; `source_value` never reaches a full window, so the
  signal returns `None` and never fires (`signal_fired == 0`,
  `action_executed == 0`) (`derived.rs:104-116`). Demonstrates the warmup gate at
  `engine.rs:498`.

`crates/graph-compiler/tests/compiler.rs`:
- `derived_reference_sets_lookback_and_requires_candles` — an `sma(window=20)`
  derived reference over `ETH`/`base` price compiles to
  `data_requirements.lookback_bars == 20` and requires the underlying ETH/base
  candle series (`compiler.rs:265-285`). Demonstrates `source_lookback` /
  `lookback_bars` extraction (`lib.rs:657,731`).

The `sma`-specific tests are the only end-to-end transform tests in
`derived.rs`; `ema`, `rolling_high`, `rolling_low`, and `roc` are exercised
through `apply_transform` but do **not** have dedicated named integration tests
in the files reviewed — flagged here so the `ema`/`rolling_*`/`roc` formulas
above are taken from the implementation (`engine.rs:512-532`), not from a passing
transform-specific assertion.
