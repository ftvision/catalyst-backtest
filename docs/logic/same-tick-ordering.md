# Same-tick ordering

When several things want to happen on **one tick** — accruals, a liquidation, a
resting limit fill, several signals firing, a chain of actions — the order they
execute in changes the result (a sell-then-buy frees balance the buy can use; a
buy-then-sell may not). For a backtest to be **reproducible** that order must be
fully deterministic, with no dependence on hash-map iteration or wall-clock. The
policy field `ordering.same_tick` is the intended knob for the *signal/action*
part of that order.

> **Status (important):** the `same_tick` enum is fully *parsed and carried* on
> the resolved policy, but the engine **does not branch on it** — there is no
> read of `policy.same_tick` anywhere in `crates/simulation-engine/src`
> (verified by search). The within-tick order described below is **hard-coded**
> and is the same for every `same_tick` value. The four variants today document
> *intent*, not divergent behavior. See "Correctness notes" for what each
> variant *would* mean and what the hard-coded order actually corresponds to.

## What it is

The enum (`crates/simulation-policies/src/lib.rs:79-82`):

```
SameTick { GraphOrder, TopologicalOrder, SignalsFirstThenActions, ConservativeAdverseOrder }
```

It is resolved from the request's `ordering.same_tick` string
(`crates/simulation-policies/src/resolve.rs:111-114`) into
`ResolvedPolicy.same_tick` (`lib.rs:138`).

| Variant | Intended meaning | Implemented? |
| --- | --- | --- |
| `graph_order` | Execute signals/actions in the order nodes appear in the graph JSON. | **No dedicated branch.** The engine's actual order is close to this for actions (graph input order) and topological for signals. |
| `topological_order` | Respect dependency edges: a node runs after the nodes it depends on. | **No dedicated branch**, but the hard-coded order *is* dependency-respecting (see below). Default for `strict_v1`/`research_v1`. |
| `signals_first_then_actions` | Evaluate every signal, then run every action. | **No dedicated branch.** The hard-coded loop already computes all signal conditions in Phase 1 before firing/acting in Phase 2 (a partial form of this). |
| `conservative_adverse_order` | Break ties in whichever order is *worst* for the trader. | **No dedicated branch.** No adverse re-ordering is applied. Default for `conservative_v1`. |

### Profile defaults

- `strict_v1` → `SameTick::TopologicalOrder` (`crates/simulation-policies/src/profiles.rs:27`)
- `conservative_v1` → `SameTick::ConservativeAdverseOrder` (`profiles.rs:44`)
- `research_v1` inherits strict's `TopologicalOrder` (`profiles.rs:51-61`, via `..strict_v1()` at `profiles.rs:59`)

Because the engine ignores the field, **all three profiles execute identically
within a tick.** The default values are accurate documentation of intent but do
not change the trace.

### The actual (hard-coded) within-tick order

The tick loop is the `for (tick_index, ts) in ticks` body inside `run`
(`run` starts at `crates/simulation-engine/src/engine.rs:180`; the loop body is
`engine.rs:238-298`). For each tick `ts`, in this exact sequence:

1. **Accrue funding** — `accrue_funding` (`engine.rs:244`).
2. **Accrue yield** — `accrue_yield` (`engine.rs:245`).
3. **Check liquidations** — `check_liquidations` (`engine.rs:246`).
4. **Snapshot tick-start equity** — `compute_equity` (`engine.rs:250`), used to
   resolve `pct_portfolio` sizing for any action this tick.
5. **Fill resting orders** — `fill_resting_orders` (`engine.rs:253-256`): limit
   orders placed on *earlier* bars get first crack at the current bar.
6. **Initial actions** (first tick only) — `run_action_chain` over
   `exec_graph.initial_actions` (`engine.rs:260-268`).
7. **Evaluate signals** — `evaluate_signals` (`engine.rs:270-290`).
8. **End-of-tick snapshot** — recompute equity, push `Snapshot` (`engine.rs:292-297`).

Within step 7, `evaluate_signals` (`engine.rs:338-466`) is itself two phases:

- **Phase 1** (`engine.rs:359-392`): compute every signal's boolean condition.
  Signals are iterated in `exec_graph.signals` order, which is the compiler's
  **topological order** (combinator inputs before the combinator that reads
  them) — applied at `crates/graph-compiler/src/lib.rs:510-512`, produced by
  `topo_order_signals` (`lib.rs:528-568`, Kahn's algorithm preserving original
  order among ready nodes). This guarantees a combinator's inputs are already
  resolved when it is evaluated.
- **Phase 2** (`engine.rs:394-465`): walk signals again in the same topological
  order; for each that fires, run its target action chains *immediately*
  (`run_action_chain`, `engine.rs:459-464`).

Action ordering:

- Top-level actions (`initial_actions`) and a signal's `targets` come from the
  compiler in **graph input order** (compiler builds `actions` by iterating
  `nodes` in graph order — `crates/graph-compiler/src/lib.rs:426-455`; `nodes`
  is graph JSON order — `lib.rs:318-325`). `ExecGraph::from_compiled` preserves
  this: `initial_actions` is a `Vec` pushed while iterating `compiled.actions`
  (`crates/simulation-engine/src/exec_graph.rs:58-80`), and a signal's `targets`
  are copied straight from the compiled signal (`exec_graph.rs:101`).
- A triggered action's **downstream chain** runs depth-first via an explicit
  stack with a `visited` set (`run_action_chain`, `engine.rs:622-708`): an
  action that fills pushes its `out_action_edges` onto the stack
  (`engine.rs:659-663`). Because it is a `stack.pop()` (LIFO, `engine.rs:637`),
  multiple downstream edges run in *reverse* push order — deterministic but not
  insertion-order.

## Which ordering / when

Since the knob is inert, there is nothing to choose between today. The
**effective** ordering is best described as: *accruals and risk first, then
prior resting orders, then this tick's new decisions, signals topologically and
actions in graph order.* This is a sensible, dependency-respecting default that
matches the intent of `topological_order`.

If you need a *specific* same-tick tie-break (e.g. force a sell before a buy on
the same bar), you must today encode it structurally — order the nodes in the
graph JSON, or chain the actions with an explicit edge — rather than relying on
the policy field.

## Correctness notes / edge cases

- **Determinism: yes, fully.** The order is fixed by source code (the tick loop
  sequence) and by the compiler's deterministic topological / graph-input
  ordering, not by `HashMap` iteration. Phase 1/2 both iterate the `Vec`
  `exec_graph.signals` (`engine.rs:364`, `engine.rs:395`); action chains use a
  `Vec`-backed stack (`engine.rs:635`). No iteration over a `HashMap` decides
  execution order. The same input always yields the same trace.
- **Look-ahead within the tick:** accruals (steps 1-2) and liquidation (step 3)
  use the *mark at `ts`* and the elapsed window `(ts - elapsed, ts]`; they run
  before any action this tick, so an action's fill cannot retroactively change
  this tick's funding/yield. Resting orders (step 5) are only eligible from the
  bar *after* placement (`order.placed_index >= tick_index` keeps it,
  `fill_resting_orders`, `engine.rs:734`), so a same-tick placement never fills
  on its own placement bar — no intra-bar look-ahead.
- **Accrual over elapsed time:** funding and yield accrue over the **actual**
  seconds since the previous tick, `elapsed_secs = ts - prev_ts`
  (`engine.rs:241`), falling back to the configured interval only on the first
  tick where there is no prior tick and no positions yet (`engine.rs:234-241`).
  Funding sums every funding point in `(ts - elapsed, ts]` via `funding_sum`
  (`accrue_funding`, `engine.rs:970`; sum at `engine.rs:988`); yield scales
  simple interest by `elapsed_secs / YEAR_SECONDS` (`accrue_yield`,
  `engine.rs:1019`; fraction at `engine.rs:1028`). This is the fix from #118: a
  tick clock coarser or gappier than the funding/accrual cadence accrues the
  whole window, not a static slice.
- **Money-conservation across same-tick actions:** every action executes on a
  *clone* of the ledger and is committed only if it fully fills
  (`run_action_chain`, `engine.rs:648-651`, "compare-and-swap"). A rejected or
  partway-failed action leaves the ledger untouched, so same-tick ordering never
  produces a half-applied action whose sibling then double-spends. `pct_portfolio`
  sizing for *all* actions this tick uses the **tick-start** equity captured in
  step 4 (`engine.rs:250`) and threaded into every action chain (`engine.rs:264`,
  `engine.rs:279`; consumed by `resolve_amount`, `engine.rs:930`), so two
  same-tick sized actions both size off the pre-tick equity rather than seeing
  each other's mid-tick effects — a deliberate, deterministic choice (it does
  mean two `pct_portfolio` buys can jointly exceed 100% of equity if balances
  allow).
- **Ordering knob is inert (limitation):** the central caveat — `same_tick` is
  parsed and round-trips through the policy contract, but no engine code reads
  it, so `graph_order`, `signals_first_then_actions`, and
  `conservative_adverse_order` produce **identical** traces to
  `topological_order`. In particular `conservative_v1`'s
  `ConservativeAdverseOrder` does **not** re-sort same-tick actions into the
  worst-case order; it is adverse only via its *other* knobs
  (`worse_side_ohlc` price selection and 25 bps slippage — `profiles.rs:41-43`).
  Tracked by [#141](https://github.com/ftvision/catalyst-backtest/issues/141) — `same_tick` is inert (the engine never branches on it).
- **Fallback / degenerate ticks:** with no market data in range the loop still
  runs one degenerate tick (`engine.rs:226-229`); ordering is unaffected.

## Tests

There is **no test that exercises `same_tick` divergent behavior**, consistent
with the field being inert — none of the engine/policy test files set
`ordering.same_tick` to a non-`None` value (`ordering: None` in
`crates/simulation-engine/tests/engine.rs:64`,
`crates/simulation-engine/tests/signals.rs:101`,
`crates/simulation-policies/tests/policies.rs:19`).

What *is* tested, and underpins the deterministic order:

- **Signal topological order** —
  `crates/graph-compiler/tests/compiler.rs::combinator_records_inputs_in_topological_order`
  (`compiler.rs:197`): asserts a combinator's input leaves are emitted
  *before* the combinator (`pos("hi") < pos("band")`, `pos("lo") < pos("band")`
  at `compiler.rs:224-225`), which is exactly the Phase-1 ordering the engine
  relies on.
- **Profile defaults round-trip (carrying the inert value indirectly)** —
  `crates/simulation-policies/tests/policies.rs::resolve_known_profiles`
  (`policies.rs:59`) asserts each profile resolves to its full `ResolvedPolicy`
  (which *includes* `same_tick`), and
  `resolved_policy_round_trips_through_json` (`policies.rs:73`) confirms a
  `ResolvedPolicy` survives a JSON round-trip — so the field is preserved
  end-to-end even though the engine ignores it. Note: the more targeted
  `strict_profile_defaults` test (`policies.rs:29-36`) does **not** assert
  `same_tick` specifically; it checks other knobs.
- **Accrual-over-elapsed-time** (the accruals that lead every tick) —
  `crates/simulation-engine/tests/funding_interval.rs` and
  `crates/simulation-engine/tests/accrual_gaps.rs` exercise funding/yield over
  variable elapsed windows (#118 behavior).
- **No intra-bar look-ahead for resting orders** —
  `crates/simulation-engine/tests/no_look_ahead.rs` and
  `crates/simulation-engine/tests/limit_orders.rs` cover next-bar eligibility,
  which is the resting-fill step (5) of the tick order.

## Related issues

- [#141](https://github.com/ftvision/catalyst-backtest/issues/141) — same_tick ordering is inert
