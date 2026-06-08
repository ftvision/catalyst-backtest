# Missing data & coverage

A backtest is only as trustworthy as the candle series under it. **Coverage** is
the question "does the required market data actually span the run, with no holes?"
and **missing-data policy** is what the engine does when it doesn't. This matters
for correctness because a silent gap either skews accrual/pricing (if ignored) or
should abort the run (if strict). The relevant knobs are `data.missing_required`
and `data.missing_optional` in the policy.

Two distinct concerns share the same gap-detection math:
- **Run-time enforcement** — `check_required_coverage` / `interior_missing` in
  `crates/simulation-engine/src/engine.rs:135` and `:123`, called once before the
  tick loop (`engine.rs:218`). This can *fail* a run.
- **Reporting** — `series_coverage` in `crates/market-data-loader/src/lib.rs:196`,
  surfaced by `coverage_response` in `crates/simulation-service/src/support.rs:85`.
  This never fails anything; it computes `completeness_pct` + `missing_ranges` for
  the coverage view.

## What it is

### Interior-gap detection (the shared rule)

Both code paths use the same definition of a "hole": given timestamps sorted
ascending and the grid `step` (= interval seconds), a gap exists wherever two
adjacent timestamps are more than one `step` apart. The count of missing buckets
in a gap is `(w[1] − w[0]) / step − 1`.

```
// engine.rs:125 — interior_missing
for w in ts_sorted.windows(2) {
    if w[1] - w[0] > step { missing += (w[1]-w[0]) as usize / step as usize - 1; }
}
```

`series_coverage` (`lib.rs:214`) is the same loop in micros, and additionally
records the inclusive `[first_missing, last_missing]` ISO range per gap
(`lib.rs:220`) and `completeness_pct = present / expected · 100`
(`lib.rs:227`), where `expected = (last − first)/step + 1` is the number of grid
buckets **between the first and last present timestamp** (`lib.rs:211`).

**Crucial framing: only *interior* gaps count.** A series that simply starts late
or ends early relative to the run window is not a hole. Both paths guard on
`len() < 2` and return clean (`engine.rs:149`, `lib.rs:198`); `expected` is
measured from first-present to last-present, never from the configured
`start`/`end`. So leading/trailing absence is invisible to this machinery.

### `missing_required` — what the engine actually does

Enum: `MissingRequired { Fail, SkipTick, ForwardFill }`
(`crates/simulation-policies/src/lib.rs:86`). "Required" = every entry in the
compiled graph's `data_requirements.candles`
(`crates/graph-compiler/src/lib.rs:132`) — the candle series the strategy's nodes
need. For each such `(venue, symbol)`, `candle_ts_in` pulls its timestamps within
`[start, end]` (`crates/simulation-engine/src/market.rs:146`) and they're checked
for interior gaps.

| Variant | Profile default | What the engine does on an interior gap |
| --- | --- | --- |
| `Fail` | `strict_v1`, `conservative_v1` | Aborts the run with `EngineError::Data("...missing bar(s) inside the window")` (`engine.rs:158`–`:159`). |
| `ForwardFill` | `research_v1` | **Pushes a warning, then runs anyway** (`engine.rs:161`). |
| `SkipTick` | (no profile sets it) | **Identical to `ForwardFill`: warning only.** |

The only branch in `check_required_coverage` is `== MissingRequired::Fail`
(`engine.rs:158`); every non-`Fail` value falls through to the same
`warnings.push(msg)` (`engine.rs:161`). See **Correctness notes** — neither name
does what it says.

Profiles: `strict_v1` → `Fail` (`crates/simulation-policies/src/profiles.rs:28`),
`research_v1` → `ForwardFill` (`profiles.rs:57`). `conservative_v1` inherits
`strict_v1`'s `Fail` via `..strict_v1()` (it doesn't override `missing_required`).

### `missing_optional` — declared, not wired

Enum: `MissingOptional { Warn, Fail, ForwardFill, FallbackProvider }`
(`simulation-policies/src/lib.rs:91`). Resolved and validated from policy JSON
(`resolve.rs:120`–`:121`) and carried on `ResolvedPolicy` (`lib.rs:140`); profiles
set it (`strict_v1` → `Warn` at `profiles.rs:29`; `conservative_v1` /
`research_v1` → `FallbackProvider` at `profiles.rs:45` / `:58`).

**But the engine never reads `missing_optional`.** A grep of
`crates/simulation-engine/src/` shows no reference to `missing_optional` or any of
its variants. So `Warn` / `Fail` / `ForwardFill` / `FallbackProvider` are all
inert at run time today — funding/gas/yield series are consumed opportunistically
(absent points simply don't accrue) regardless of this setting.

### `completeness_pct` + `missing_ranges` (reporting)

`series_coverage` → `SeriesCoverage` (`lib.rs:182`): `present`, `expected`,
`completeness_pct`, `missing`, `missing_ranges`, `first`, `last`.
`coverage_response` (`support.rs:85`) runs it over every candle/funding/gas/yield
series in the bundle and emits per-series rows with `complete` (`n > 0 && missing
== 0`, `support.rs:93`), `completeness_pct`, `missing`, `missing_ranges`,
`start`, `end`. This is a *report* — it never blocks a run and is independent of
the `missing_required` policy.

Note there is also a coarser provider-level `Coverage { start, end, complete }`
(`crates/contracts/src/market_data.rs:95`), produced by the loader's
`coverage` closure (`market-data-loader/src/lib.rs:643`) as `start..end` window
metadata where `complete` just means the series is non-empty, not interior-gap
analysis. The interior detail lives only in `SeriesCoverage`.

## When does enforcement run / which to use

`check_required_coverage` runs **once, before the tick loop** (`engine.rs:218`),
not per tick. It's a precondition check on the required candle series.

- **`Fail` (strict/conservative)** — production-grade runs where a hole in a
  required price series would silently corrupt results. Prefer this when you need
  to *trust* the P&L.
- **`ForwardFill` (research)** — quick exploration where you'd rather get a result
  with a warning than an abort. **Caveat:** today this does not actually
  forward-fill bars (see below); it just suppresses the abort.
- **`SkipTick`** — intended for "drop the gapped ticks and keep going", but is
  currently not distinguishable from `ForwardFill`.

## Correctness notes / edge cases

- **Look-ahead: none.** Coverage is a structural property of timestamps; it reads
  no prices and makes no forward-looking decision. The check runs on the already-
  loaded window before any tick executes.

- **Interior-only by design — leading/trailing absence is silent.** Because
  `expected` is measured first-present → last-present and `len() < 2` short-
  circuits, a series that covers only the back half of `[start, end]` reports
  100% complete with zero missing. A strategy that needed data from `start` will
  *not* be failed by `missing_required`; it just runs on fewer ticks. If you need
  full-window coverage you must check `first`/`last` against the run window
  yourself — neither the engine nor `completeness_pct` does it for you.

- **`SkipTick` and `ForwardFill` are not implemented as named.** Both are pure
  warnings (`engine.rs:158`–`:161`); the engine does not skip the gapped ticks nor
  synthesize/carry-forward a bar for the missing buckets. The tick clock is simply
  whatever timestamps the bundle contains (`market.rs:119`, `ticks`), so a gap
  just means no tick fires in the hole. Functionally, under any non-`Fail` value
  the run proceeds over the present ticks with a warning. The enum richness is
  forward-looking; document/treat `ForwardFill` and `SkipTick` as "warn and
  continue" today.

- **`missing_optional` is inert.** All four variants resolve and validate but are
  never consulted by the engine. Optional data (funding/gas/yield) absence is
  handled by the accrual code simply finding no point for that tick, not by this
  policy. Do not rely on `Fail`-on-missing-optional or `FallbackProvider`
  substitution at run time — neither happens here.

- **Money conservation: not this layer's job.** Coverage gates *whether* a run
  proceeds; it moves no funds. The settlement/accrual invariants live elsewhere.
  The one indirect correctness link: because accrual scales by *actual* elapsed
  seconds between ticks (`ts − prev_ts`, `engine.rs:241`), a gap that survives a
  non-`Fail` policy yields a longer `elapsed_secs` across the hole rather than a
  dropped accrual — funding/yield are integrated over the real gap, not silently
  zeroed. (This is the elapsed-time accrual behavior; see the funding/yield docs.)

- **Coarse interval hides sub-interval holes.** Gap detection is entirely relative
  to `step = interval_seconds(interval)`. Two timestamps within one `step` are
  never a gap, even if the underlying source skipped finer bars. Run a 1h backtest
  over data that's really missing some 5m bars and coverage will read clean — the
  grid it's checked against is the *configured* interval, not the data's native
  resolution. Conversely a finer configured interval surfaces more holes. Also:
  `step <= 0` (an unknown/zero interval) short-circuits the engine check to `Ok`
  (`engine.rs:144`) and reports 100% (`lib.rs:198`–`:202`), so an invalid interval
  disables gap detection rather than erroring here. (In `run`, an unknown interval
  is actually rejected earlier at `engine.rs:189`–`:190`; the `step <= 0` guard is
  a defensive belt-and-braces.)

- **Determinism.** Inputs are sorted, de-duplicated timestamps; the window loops
  are order-stable; `completeness_pct` is a pure ratio. Same bundle + interval →
  identical coverage and identical fail/warn decision every run.

- **`missing_ranges` brackets the gap, not the present bars.** The reported range
  is `[first_missing, last_missing]` — exclusive of the bracketing present bars:
  `(w[0] + step, w[1] − step)` (`lib.rs:220`). For a single missing bucket this
  collapses to that one timestamp on both ends (see the test asserting
  `02:00 … 02:00`).

## Tests

`crates/simulation-engine/tests/coverage_gaps.rs` (engine enforcement):
- `strict_fails_on_interior_gap` — hours 0,1,3,4 present (hour 2 missing) under
  `strict_v1` (`Fail`) → run errors with a message containing "missing".
- `research_warns_on_interior_gap` — same hole under `research_v1` (`ForwardFill`)
  → run succeeds, trace carries a "missing bar" warning.
- `contiguous_series_runs_clean` — hours 0–4 all present → no "missing bar"
  warning. (Together these demonstrate that `ForwardFill` only warns; there is no
  separate `SkipTick` test because it shares the branch.)

`crates/market-data-loader/tests/loader.rs` (the coverage math):
- `series_coverage_contiguous_is_complete` — 5 contiguous hours → `missing 0`,
  `completeness_pct 100`, empty `missing_ranges`.
- `series_coverage_detects_interior_hole` — hours 0,1,3,4 → `present 4`,
  `expected 5`, `missing 1`, `completeness_pct 80`, range `02:00…02:00`.
- `series_coverage_multi_bucket_gap` — hours 0 then 4 → `missing 3`, range
  `01:00…03:00`.

`crates/simulation-service/tests/service.rs` (the coverage endpoint):
- `coverage_from_inline_bundle` — contiguous candles → `complete true`,
  `missing 0`, `completeness_pct 100`.
- `coverage_reports_interior_gaps` — candles at hours 0,2 (hour 1 missing) →
  `complete false`, `missing 1`, `completeness_pct ≈ 66.7`,
  `missing_ranges[0][0] == 01:00`.
