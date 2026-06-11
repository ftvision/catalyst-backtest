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

**Crucial framing: the two paths now diverge on leading/trailing absence.** The
*reporting* path (`series_coverage`) still measures `expected` from
first-present to last-present, never from the configured `start`/`end` — a
late-starting series reports 100% complete there. The *enforcement* path was
fixed in #167: `check_required_coverage` also checks each required series
against the **requested** `[start, end]` —
- a series with **zero points** in the window is "required candles
  {venue}/{symbol} have no data in the requested window start..end" (before
  #167 it passed silently through the `len() < 2` guard);
- a series that starts late / ends early is "required candles {venue}/{symbol}
  cover first..last but the requested window is start..end (N leading / M
  trailing missing bar(s))", with `leading = (first − start) / step` and
  `trailing = (end − last) / step` (integer division, so a sub-step anchor
  offset doesn't count as a missing bar).

Both are routed through the same `missing_required` switch as interior gaps:
abort under `fail`, warning otherwise.

### `missing_required` — what the engine actually does

Enum: `MissingRequired { Fail, Warn, SkipTick, ForwardFill }`
(`crates/simulation-policies/src/lib.rs`). "Required" = every entry in the
compiled graph's `data_requirements.candles`
(`crates/graph-compiler/src/lib.rs:132`) — the candle series the strategy's nodes
need. For each such `(venue, symbol)`, `candle_ts_in` pulls its timestamps within
`[start, end]` (`crates/simulation-engine/src/market.rs:146`) and they're checked
for zero coverage, leading/trailing absence vs the requested window (#167), and
interior gaps (#42).

| Variant | Profile default | What the engine does on a coverage shortfall |
| --- | --- | --- |
| `Fail` | `strict_v1`, `conservative_v1` | Aborts the run with `EngineError::Data("...missing bar(s) inside the window")` (`engine.rs:179`–`:180`). |
| `Warn` | `research_v1` | **Pushes a warning, then runs anyway** (`engine.rs:182`) — the variant is named for exactly what it does. |
| `SkipTick` / `ForwardFill` | (none) | **Rejected at policy validation** (#159, implement-or-reject) — they previously parsed and silently behaved as warn-and-continue. |

The only branch in `check_required_coverage` is `== MissingRequired::Fail`
(`engine.rs:179`); `Warn` falls through to `warnings.push(msg)`. `SkipTick` and
`ForwardFill` cannot reach the engine: `validate` refuses them until skipping
ticks / synthesizing bars is actually implemented (#159).

Profiles: `strict_v1` → `Fail` (`crates/simulation-policies/src/profiles.rs:28`),
`research_v1` → `Warn` (`profiles.rs:60`). `conservative_v1` inherits
`strict_v1`'s `Fail` via `..strict_v1()` (it doesn't override `missing_required`).

### `missing_optional` — only `warn` is accepted

Enum: `MissingOptional { Warn, Fail, ForwardFill, FallbackProvider }`
(`simulation-policies/src/lib.rs`). The engine never reads this field —
funding/gas/yield series are consumed opportunistically (absent points simply
don't accrue). `Warn` names that de-facto behavior and is the only value
`validate` accepts; `Fail` / `ForwardFill` / `FallbackProvider` are **rejected
at policy validation** (#142, implement-or-reject) instead of being silently
inert. All three profiles now declare `Warn`.

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

- **Leading/trailing absence is enforced and disclosed (#167, fixed).** A
  required series that covers only part of `[start, end]` — or none of it — now
  fails the run under `missing_required = fail` and warns otherwise, with both
  the covered and requested windows named in the message. Independently, the
  trace (and result metadata) always carry `effective_start`/`effective_end` =
  the first/last actual tick of the run, set even when they equal the requested
  window, so "the run you got" is never inferred from silence. The reporting
  path (`completeness_pct`) is still first-present → last-present; the
  enforcement path is the one that compares against the requested window.

- **Honesty note: "listed mid-window" looks identical to "data missing".** From
  timestamps alone the engine cannot distinguish an asset that didn't exist
  before its listing date from a provider that lost the leading bars. #167
  deliberately does **not** guess: it discloses the shortfall either way. If the
  asset genuinely lists mid-window, shorten the requested window (or run a
  warn-policy profile and read the effective window from the metadata) — don't
  expect the engine to silently absorb the difference.

- **`SkipTick` and `ForwardFill` are rejected until implemented (#159).** The
  engine does not skip gapped ticks nor synthesize/carry-forward bars; the tick
  clock is simply whatever timestamps the bundle contains (`market.rs:119`,
  `ticks`), so a gap means no tick fires in the hole. Use `warn` for
  warn-and-continue or `fail` to abort; the unimplemented names can no longer be
  selected and silently mean something else.

- **`missing_optional` accepts only `warn` (#142).** Optional data
  (funding/gas/yield) absence is handled by the accrual code simply finding no
  point for that tick. The other variants (`fail` / `forward_fill` /
  `fallback_provider`) are rejected at validation until they exist.

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
- `research_warns_on_interior_gap` — same hole under `research_v1` (`Warn`)
  → run succeeds, trace carries a "missing bar" warning.
- `contiguous_series_runs_clean` — hours 0–4 all present → no "missing bar"
  warning.
- `unimplemented_policy_values_are_rejected_not_ignored`
  (`crates/simulation-policies/tests/policies.rs`) pins the `skip_tick` /
  `forward_fill` / non-warn `missing_optional` rejections.

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

## Related issues

- [#142](https://github.com/ftvision/catalyst-backtest/issues/142) — missing_optional variants beyond `warn` (rejected until implemented)
