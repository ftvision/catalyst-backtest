# Signals & firing control

A **signal** is a boolean condition evaluated each tick from market data; when it
**fires** it runs its downstream action chain. "Firing control" is the set of
policy knobs that decide *when* a true condition counts as a fire — the
difference between "buy every bar the price is below $1800" and "buy once, the
moment it first crosses below." Getting this right is a correctness concern: the
wrong knob silently multiplies (or suppresses) every trade the strategy makes.

Two layers cooperate:
- **The condition** — what makes a signal `true` this tick. Computed in
  `crates/simulation-engine/src/engine.rs` `evaluate_signals` (Phase 1,
  `engine.rs:359-392`), with the leaf comparison in
  `crates/simulation-engine/src/exec_graph.rs` `eval_threshold` (`exec_graph.rs:130-140`).
- **The firing semantics** — which of those `true` ticks actually fire. Driven by
  `policy.signal_trigger`, `policy.repeat`, `policy.cooldown`, `policy.repeat_max_count`
  in `evaluate_signals` Phase 2 (`engine.rs:394-465`).

## What it is

### The condition (Phase 1)

Every signal in the exec graph is evaluated once per tick, in compiler-emitted
**topological order**, so a combinator's inputs are already resolved when it is
read (`engine.rs:364-392`). Two signal kinds (`exec_graph.rs:22-28`):

| Kind | Rule | Code |
| --- | --- | --- |
| `Threshold` (leaf) | `eval_threshold(lhs, operator, rhs)` over a market-data `Source` (price/funding/yield/gas/derived) vs. a `Reference` (`const`/`source`/`var`) | `engine.rs:366-380`, `exec_graph.rs:130-140` |
| `Combinator` | `all` = every input true, `any` = some input true, `not` = first input negated | `engine.rs:381-389` |

`eval_threshold` supports `<`, `<=`, `>`, `>=`, `==`, `!=`; any other operator
string evaluates to `false` (`exec_graph.rs:131-139`).

A leaf whose `source` or `reference` has **no data this tick** yields condition
`None` (a warning is pushed) rather than `false` (`engine.rs:374-378`). A
combinator reading a `None` input treats it as `false` via
`.flatten().unwrap_or(false)` (`engine.rs:382`).

### The firing semantics (Phase 2)

Only signals with at least one target run Phase 2 (`engine.rs:396-398`). A signal
whose condition is `None` this tick is skipped **without touching crossing state**
— `signal_state` is left unchanged so a data gap can't fake a future crossing
edge (`engine.rs:399-403`). Otherwise the condition is recorded into
`signal_state` for next tick's crossing comparison (`engine.rs:404-405`).

A fire then passes four sequential gates (`engine.rs:407-446`); failing any one
`continue`s without firing.

**1. Trigger edge — `policy.signal_trigger`** (`engine.rs:408-411`):

| `signal_trigger` | Edge condition | Meaning |
| --- | --- | --- |
| `level` | `condition` | fires on **every** tick the condition holds |
| `crossing` | `condition && !previous` | fires only on the **false→true transition** |
| `crossing_with_cooldown` | `condition && !previous` | crossing edge, plus the cooldown gate (4) |
| `once_per_backtest` | `condition` (level edge) + gate (2) | the **first** holding tick, ever |

Default profile (`strict_v1`) is `crossing` (`profiles.rs:23`).

**2. `once_per_backtest` gate** (`engine.rs:417-421`): if the trigger is
`once_per_backtest` and the signal is already in `ever_fired`, skip. Combined with
the `level` edge this means "first tick the condition is true, then never again."

**3. Repeat gate — `policy.repeat`** (`engine.rs:424-432`), using a per-signal
`fire_count`:

| `repeat` | Rule | Code |
| --- | --- | --- |
| `never` | `count == 0` — at most one fire ever | `engine.rs:426` |
| `on_each_signal_fire` | always allowed | `engine.rs:427` |
| `with_cooldown` | always allowed here; throttled by gate (4) | `engine.rs:427` |
| `max_count` | `count < repeat_max_count` (falls back to `count == 0` if the cap is unset) | `engine.rs:428` |

Default profile is `on_each_signal_fire` (`profiles.rs:24`).

**4. Cooldown gate — `policy.cooldown`** (`engine.rs:434-446`): active when the
trigger is `crossing_with_cooldown` **or** the repeat is `with_cooldown`. If a
prior fire exists and `ts - last_fired < cooldown_secs`, skip. Duration strings
(`30s`/`15m`/`1h`/`2d`) are parsed by `parse_duration_secs` (`engine.rs:552-564`).

If all four gates pass, the signal fires (`engine.rs:448-465`): `ever_fired`,
`fire_count`, and `last_fired` are updated, a `signal_fired` event is emitted, and
**each target's action chain runs immediately, inline, on this same tick** via
`run_action_chain` (`engine.rs:459-464`).

## When the actions actually run within a tick

Order inside one tick (`run`, `engine.rs:238-298`):
1. funding accrual, yield accrual, liquidation checks (`engine.rs:244-246`)
2. resting limit orders from earlier bars fill/expire (`engine.rs:253-256`)
3. one-time `initial_actions` (first tick only, `engine.rs:260-268`)
4. **`evaluate_signals`** — conditions computed, then firing → action chains run
   (`engine.rs:270-290`)
5. end-of-tick equity snapshot (`engine.rs:292-297`)

So a market-order action triggered by a fire settles **the same tick the signal
fires**, against that tick's bar. A *limit* action triggered by a fire only
**rests** this tick and becomes eligible from the next bar onward (`engine.rs:674-705`,
`734`) — that next-bar eligibility is a look-ahead guard, not a signals concern.

## Which to use / why choose one over another

- **`level`** — "stay-in-state" strategies: hold the position for as long as the
  condition is true (e.g. keep depositing into yield while APR ≥ 5%). Re-fires
  every qualifying tick, so it is almost always paired with idempotent or
  position-aware actions, or with a `repeat`/cooldown cap. The yield/funding tests
  use `level` and expect one fire **per** qualifying tick (`signals.rs:156`,
  `188`).
- **`crossing`** — event strategies: act once on the transition, not every bar the
  condition persists. The price-buy tests use `crossing` and get exactly one fire
  across a multi-bar dip (`signals.rs:245-247`).
- **`crossing_with_cooldown`** — crossing, but debounced: ignore re-crossings that
  happen too soon (avoids whipsaw in a choppy series).
- **`once_per_backtest`** — a single lifetime action (e.g. initial allocation gated
  on a condition rather than on tick 0).
- **`repeat` / `cooldown` / `max_count`** are orthogonal throttles layered on top of
  the trigger: cap total fires (`never`, `max_count`), or rate-limit them in time
  (`with_cooldown`).

## Correctness notes / edge cases

- **Look-ahead: none.** Conditions read only `source_value`/`reference_value` at
  the current `ts` (`engine.rs:367-370`); `Derived` sources sample backward over
  the trailing window and require a full warmup before becoming valid
  (`engine.rs:486-502`). Crossing compares against `previous` state captured on a
  *prior* tick. No future bar is consulted.
- **Cooldown boundary is inclusive (#121).** The skip test is strict-less-than:
  `if ts - last < cd { continue }` (`engine.rs:442`). A fire is therefore **allowed
  exactly when `ts - last_fired == cooldown_secs`** — the cooldown is "at least
  this long," not "strictly longer." A 3h cooldown re-permits a fire on the bar 3h
  later, not 4h later.
- **Gate ordering matters.** Trigger edge → once_per_backtest → repeat → cooldown
  (`engine.rs:408-446`). Because the edge is checked first, with `crossing` a
  condition that stays true across many bars only consumes one fire (and thus one
  cooldown/`max_count` slot) at the transition; under `level` every true bar is a
  fire attempt that the repeat/cooldown gates must then throttle.
- **Data gaps don't corrupt crossing state.** A `None` condition skips the signal
  and leaves `signal_state` untouched (`engine.rs:399-403`), so the **next** real
  observation is compared against the last *real* state, not a phantom `false`.
- **`fire_count` is incremented only on an actual fire** (`engine.rs:450`), so
  suppressed ticks (failed edge/cooldown) don't burn a `max_count` slot.
- **`last_fired` is set on every fire** (`engine.rs:451`), so the cooldown clock
  restarts from the most recent fire, not the first.
- **Determinism.** Signals are iterated in the compiler's fixed topological order
  and all state is keyed by stable signal id (`engine.rs:364`, `395`), so a given
  input bundle + policy always produces the identical fire sequence.

### Known limitations

- **No per-signal firing config.** `signal_trigger`/`repeat`/`cooldown`/`max_count`
  are **global policy knobs** (`ResolvedPolicy`, `lib.rs:133-137`), applied
  uniformly to every signal in the graph (`engine.rs:408`, `425`, `435`). You
  cannot make one signal `level` and another `crossing` in the same run.
- **`SignalTrigger` has no per-tick "debounce/persistence" variant.** The enum is
  exactly `{ level, crossing, crossing_with_cooldown, once_per_backtest }`
  (`lib.rs:69-72`); there is no "condition must hold for N bars" option.
- **Cooldown without a prior fire never blocks.** The gate only engages when
  `last_fired` already has an entry (`engine.rs:438-441`); the very first fire is
  never delayed.
- **Validation couples the cooldown knobs.** `crossing_with_cooldown` requires
  `signals.cooldown`, `with_cooldown` requires `signals.cooldown`, and `max_count`
  requires `signals.max_count`, else `resolve_policy` rejects the policy
  (`resolve.rs:180-195`). A malformed cooldown string that fails `parse_duration_secs`
  is **silently treated as no-cooldown** at the firing site (`engine.rs:438-439`),
  not an error.

## Tests

`crates/simulation-engine/tests/signals.rs`:
- `threshold_price_source_matches_price_threshold` — `crossing` trigger fires once
  over a multi-bar dip (`signals.rs:227-248`).
- `crossing_with_cooldown_suppresses_a_refire` — closes `["1700","2000","1700",
  "2000","1700"]` give would-be crossings into "below 1800" at ticks 0, 2, 4; with
  a `3h` cooldown the tick-2 crossing (2h after the tick-0 fire, < 3h) is
  suppressed and the tick-4 crossing fires → 2 fires (`signals.rs:275-287`). The
  surviving crossing lands 4h after the last fire, so this test exercises a
  comfortably-past-cooldown re-fire, **not** the exact `== cooldown_secs` boundary
  of #121 — no test currently lands a fire precisely on that boundary.
- `repeat_never_fires_at_most_once` — two crossings, `repeat=never` → 1 fire
  (`signals.rs:289-300`).
- `repeat_max_count_caps_fires` — three crossings, `max_count=2` → 2 fires
  (`signals.rs:302-313`).
- `funding_source_threshold_reads_funding_and_fires` /
  `yield_source_threshold_reads_apr_and_fires` — `level` trigger fires once **per**
  qualifying tick (2 of 3) (`signals.rs:130-157`, `159-189`).
- `all_combinator_fires_only_when_every_input_true` /
  `any_combinator_fires_when_some_input_true` / `not_combinator_inverts_its_input`
  — combinator condition logic under `level` (`signals.rs:328-403`).
- `reference_var_resolves_from_graph_variables` — `Reference::Var` RHS resolves a
  threshold (`signals.rs:250-271`).
- `yield_source_without_candles_drives_ticks` — signals fire and chain actions even
  when a non-candle (yield-only) series drives the tick clock (`signals.rs:191-223`).
