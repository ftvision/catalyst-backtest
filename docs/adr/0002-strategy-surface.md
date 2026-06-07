# ADR 0002 — Strategy surface: observable-parameterized signals, firing control, composition

- **Status:** Accepted — steps 1 & 4 implemented (steps 2–3 pending)
- **Builds on:** ADR 0001 (Rust owns the run/service path)
- **Relates to:** #28 (compiler now single-source in Rust), reviewer feedback on
  "any runnable graph" support (more signal types, repeat/cooldown, variables)

## Context

The strategy-authoring surface today is deliberately minimal. A graph is nodes
(`action` | `signal`) plus edges. The only signal is `price_threshold`
(`symbol` `operator` `threshold`, with `operator ∈ {< <= > >= == !=}`). Actions
are `swap`, `perp_order`, `yield_deposit`, `yield_withdraw` with **static**
amounts. Composition is implicit: edges into signals are dropped
(`graph-compiler/src/lib.rs:233-240`); a signal fans out to its target actions.

This expresses hand-placed price grids/ladders and fixed-level perp swings, but
not the systematic, data-driven strategies that are common — and crypto-native —
in practice: **funding-rate carry**, **yield rotation**, trend/mean-reversion on
**derived series** (moving averages, breakouts), and anything needing
**risk-relative sizing** (stops, take-profit, rebalancing).

Three facts make now the right time to design this:

1. **The compiler is single-source in Rust** (`crates/graph-compiler`; the Python
   one is conformance-only). A new primitive is one language, not two.
2. **The engine is already shaped like a general model, hardwired to price.** A
   `Signal` is `{ id, symbol, operator, threshold, targets }` — "read a scalar,
   compare to a threshold, emit a bool" — that always reads
   `index.price_any(symbol, ts)`.
3. **The workbench API derives from `compile()`.** `/backtests/preview` returns
   `graph_summary` + `data_requirements`; `/market-data/coverage` keys off
   `data_requirements`. Anything that participates in `data_requirements` shows
   up in preview and coverage **for free**.

### Adjacent reviewer items and how they relate

- **Repeat/cooldown in the engine** — the policy layer declares and *validates*
  `repeat` and `cooldown` (`simulation-policies/src/resolve.rs:176-184`), but the
  engine ignores them: `Crossing` and `CrossingWithCooldown` are evaluated
  identically and `repeat` is never read. This is the **firing-control axis of
  this design** and lives in the exact function the observable refactor rewrites
  (`evaluate_signals`). **In scope here.**
- **Variables/settings** — parameterized strategies want `threshold = $var`.
  Handled as a `Reference` source (below). **In scope here.**
- **Live data adapters** (Base gas, Aave/yield, fallback prices) — an *adjacent
  dependency*, not part of this surface: per ADR 0001 those are Python ingestion
  that writes the store; these signals are Rust that reads it. The constraint
  this design imposes is that `Source` kinds map 1:1 onto the store's data kinds.
  **Out of scope, but coordinated.**
- **EVM swap realism** and **true limit-order semantics** — execution-models /
  fill quality. A limit order is the conceptual cousin of a price-threshold
  signal ("rest until price hits a level, then fill"), but it belongs in
  execution (resting orders, partial fills via `order_type:"limit"`), **not** the
  signal layer. Flagged so we don't build two mechanisms for the same idea.
  **Out of scope.**

## Decision

Model a strategy as four orthogonal axes — **observe → reduce → fire → act** —
plus a **compose** layer over the booleans. Parameterize the *observable* instead
of multiplying signal subtypes.

### 1. Observe — a `Source` (the scalar a signal reads)

```rust
enum Source {
    Price   { symbol: String, venue: Option<String> },
    Funding { venue: String, symbol: String },
    Yield   { protocol: String, asset: String, chain: String, pool: Option<String> },
    Gas     { chain: String },
    // future, additive — a transform over another Source:
    // Derived { of: Box<Source>, transform: Sma|Ema|RollingHigh|RollingLow|Roc, window: u32 },
}
```

A `Source` has exactly **two consumers**, both already one-line dispatches:

```rust
// compiler: what data does this need?  (generalizes the Typed::Threshold arm, lib.rs:383)
fn requirement_of(s: &Source) -> Req            // Price→candle, Funding→funding, Yield→yield, Gas→gas

// engine: what is its value this tick?  (replaces the hardwired price_any call)
fn value_of(s: &Source, idx: &BundleIndex, ts: i64) -> Option<Decimal>
//   Price→price_any, Funding→funding_at, Yield→apr_at, Gas→gas_at   (all already on BundleIndex)
```

Adding a future data source is **one arm in two functions**, never a new node
type. This converts "N data kinds × M signal shapes" into "N sources + M shapes,
composed," and keeps data-requirements and engine reads from one description so
they cannot drift.

### 2. Reduce — a comparison against a `Reference`

```rust
enum Reference {
    Const(Decimal),       // today's static threshold
    Source(Source),       // crossover / relative: price vs MA, funding vs 0, ...
    Var(String),          // resolve from graph `variables` — parameterized strategies
}

struct ThresholdConfig { source: Source, operator: Operator, reference: Reference }
```

`price_threshold` is preserved as **sugar** that desugars to
`{ source: Price, operator, reference: Const }` — existing graphs, fixtures, and
UI keep working unchanged.

### 3. Fire — firing control (folds in the repeat/cooldown gap)

Firing semantics are **orthogonal to the observable** and apply to every signal
type. This is where the engine is brought in line with the policy contract it
already validates:

- `trigger`: `level` (true while condition holds) | `crossing` (false→true) |
  `once_per_backtest`.
- `cooldown`: minimum time between fires — **honor it** (currently a no-op). Needs
  per-signal `last_fired_ts` state.
- `repeat`: `never` | `on_each_signal_fire` | `with_cooldown` | `max_count` —
  **implement it** (currently unread). `max_count` needs a per-signal fire
  counter.

Because the `Source` refactor already rewrites `evaluate_signals`, the per-signal
fire state (`last_fired_ts`, `fire_count`) is added in the same pass — the
function is touched once, not twice.

### 4. Compose — boolean combinators (the one structural change)

```rust
// new signal subtypes whose inputs are other signals
All / Any / Not
```

This requires the only change to graph *semantics*: **signal→signal edges become
meaningful for combinator subtypes** (today all edges into signals are dropped).
The compiler then topologically orders signals (leaves first, combinators after);
a combinator targets actions like any other signal, reusing trigger derivation.
All other parts of this design are purely additive — new enum variants and match
arms, no change to existing graphs.

### 5. Act — relative action sizing

```rust
enum Amount {
    Absolute(Decimal),                                  // bare string today — back-compatible
    Relative { basis: PctPortfolio|PctPosition|PctBalance, value: Decimal },
}
```

Resolved at execution time against the ledger. Unlocks stop-loss, take-profit,
and rebalancing without new action subtypes.

## How it threads the workbench surface

- **`/backtests/preview`** — `graph_summary` + `data_requirements` come from
  `compile()`, so they extend automatically; only addition is surfacing the
  `Source` kind in the signal summary for UI labeling.
- **`/market-data/coverage`** — keys off `data_requirements`; funding/yield
  signals now declare those reqs, so coverage rows appear with no coverage-code
  changes.
- **`/backtests/{id}/events`** — new signals emit the same `signal_fired` event;
  put the observed value + threshold in `detail` (e.g. `funding=0.0003 ≥
  0.0001`) and the paginated Explain/audit timeline renders it unchanged.
- **policy** — `trigger`/`repeat`/`cooldown` are reused for every signal type.

## Backward compatibility

All of steps 1–3 and 5 are additive: new `Source`/`Reference`/`Amount` variants,
new match arms, `price_threshold` and bare-string amounts preserved as sugar.
Existing `catalyst.graph.definition.v1` graphs stay valid. Only step 4
(composition) changes graph semantics, and only by *allowing* previously-dropped
edges — no existing graph changes meaning.

## Migration sequencing (each shippable)

1. **`Source` + generalized `threshold`; wire funding + yield; honor
   repeat/cooldown.** Reuses existing `data_requirements` plumbing and
   `BundleIndex` reads; rewrites `evaluate_signals` once. Unlocks carry and
   rotation. (Depends on funding/yield data existing in the store — coordinate
   with the data-adapter work.)
2. **Relative action sizing** (`Amount`). Unlocks stops / take-profit /
   rebalancing.
3. **Derived sources** (`sma`/`ema`/`rolling_high|low`/`roc`). Adds rolling
   state **and** the easy-to-miss requirement: `data_requirements` must carry each
   signal's max **lookback** so the loader fetches `start − lookback`, or the
   first N bars are blind.
4. **Composition** (`all`/`any`/`not`) — the structural step (signal→signal
   edges + topological evaluation).

## Implementation status

**Step 1 is implemented** (additive, no existing graph changes meaning):

- `Source` (`price`/`funding`/`yield`/`gas`), `Reference` (`const`/`source`/`var`),
  and the `threshold` subtype in `catalyst-contracts`; `price_threshold` is
  desugared to a price `Source` + const `Reference` by the compiler.
- The Rust compiler derives data requirements from a signal's `Source` (and a
  `Reference::Source`), so `/backtests/preview` and `/market-data/coverage` cover
  funding/yield signals automatically.
- The engine reads any `Source` per tick (reusing `BundleIndex`
  `price_any`/`funding_at`/`apr_at`/`gas_at`) and **honors repeat/cooldown**:
  `cooldown` (last-fired gate), `repeat = never|on_each_signal_fire|with_cooldown|
  max_count` (with a new `signals.max_count` policy field), and resolves
  `Reference::Var` from `Graph.variables`.
- Schemas (`graph`, `simulation-policy`) updated; a `graph.threshold-funding.json`
  example is round-trip validated cross-language. Rust + Python test suites green.

The Python conformance compiler was intentionally **not** extended for the new
subtypes — per ADR 0001 the Rust compiler is authoritative; the Python mirror
stays scoped to the existing `price_threshold` golden graphs.

**Step 4 (composition) is also implemented:**

- `all` / `any` / `not` combinator subtypes. Their inputs are the upstream
  signals with an edge into them — the compiler now **keeps signal→signal edges
  for combinator targets** (still drops edges into leaf signals) and records them
  on `CompiledSignal.inputs`.
- The compiler emits signals in **topological order** (Kahn) so a combinator's
  inputs resolve first, and **rejects cycles** plus bad arity (`not` needs
  exactly one input; `all`/`any` need ≥1).
- The engine evaluates signals in **two phases**: compute every signal's boolean
  condition (leaves from data, combinators from their inputs), then apply the
  firing semantics (trigger/repeat/cooldown) to signals that drive actions.
- `signal_fired` detail reports `op`/`inputs`/`result` for combinators.
- Schema (`graph`) gains the three subtypes; a `graph.threshold-composed.json`
  example is round-trip validated cross-language.

Steps 2 (relative sizing — execution-models + portfolio valuation) and 3
(derived sources `sma`/`ema`/`rolling_high|low`/`roc` + warmup/lookback
propagation through the loader) remain; both open cross-cutting surfaces beyond
the signal layer.

## Open decisions

- **Generalize vs. discrete subtypes.** Recommended: generalize
  (`Source`-parameterized `threshold`), keep `price_threshold` as sugar. The
  single-source-Rust compiler made this cheap and it avoids the combinatorial
  subtype tax.
- **Event-driven vs. state-driven actions** (the deeper fork). The engine is
  *event*: "signal fires → run action once." Carry, stops, and rebalancing are
  naturally *state*: "hold target exposure *while* condition holds," "exit *if*
  breached." Approximated today with enter/exit signal pairs (the `g13`
  perp-swing fixture does this by hand). The honest long-term model is a
  `target`/guard node — named here, not built; it is a larger change than the
  whole signal-vocabulary expansion.
- **Limit-order boundary.** Keep `order_type:"limit"` in execution-models, not as
  a signal — even though it overlaps conceptually with price thresholds.

## Alternatives considered

- **Add discrete subtypes per data kind** (`funding_rate_threshold`,
  `yield_threshold`, …). Rejected: combinatorial, and still cannot express
  source-vs-source comparisons (MA cross). The `Source` union subsumes it.
- **A full free-form expression DSL.** Rejected for now: large frontend lift
  (the preview API returns flat `signals`/`actions` lists), more than "a bit
  more" of vocabulary. The curated catalog over a shared observable evaluator
  gets most of the value; the expression form can come later if demanded.
