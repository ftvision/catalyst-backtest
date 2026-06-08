//! The deterministic tick/event loop.

use std::collections::{BTreeMap, HashMap, HashSet};

use rust_decimal::Decimal;
use serde_json::{json, Value};

use catalyst_contracts::graph::{
    Amount, AmountBasis, PerpOrderConfig, Reference, Source, SwapConfig, Transform, YieldConfig,
};
use catalyst_contracts::trace::{Event, Portfolio, SimulationTrace, Snapshot};
use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_execution_models::{
    execute_perp, execute_perp_at, execute_swap, execute_swap_at, execute_yield_deposit,
    execute_yield_withdraw, is_stable, limit_fill_price, place_perp_limit, place_swap_limit,
    Execution, Fill, LimitPlacement, MarketContext, PlacedLimit,
};
use catalyst_portfolio_ledger::{Ledger, PerpPosition, PerpSide, YieldPosition};
use catalyst_simulation_policies::{
    resolve_policy, Funding, InsufficientBalance, LiquidationCheck, MissingRequired, Repeat,
    ResolvedPolicy, SignalTrigger,
};

use crate::exec_graph::{eval_threshold, ActionNode, CombinatorOp, ExecGraph, Signal, SignalDef};
use crate::market::{format_ts, parse_ts, BundleIndex, TickContext};

const YEAR_SECONDS: i64 = 31_536_000;

/// Everything the engine needs to run a simulation. The engine reads only this;
/// it never fetches raw market data.
pub struct SimulationInput {
    pub graph: Graph,
    pub config: BacktestConfig,
    pub policy: SimulationPolicy,
    pub market_data: MarketDataBundle,
}

#[derive(Debug)]
pub enum EngineError {
    Policy(String),
    Config(String),
    Data(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Policy(m) => write!(f, "policy error: {m}"),
            EngineError::Config(m) => write!(f, "config error: {m}"),
            EngineError::Data(m) => write!(f, "data error: {m}"),
        }
    }
}
impl std::error::Error for EngineError {}

// --- limit / resting orders ---

/// The config a resting order replays when it fills.
enum RestingKind {
    Swap(SwapConfig),
    Perp(PerpOrderConfig),
}

impl RestingKind {
    fn label(&self) -> &'static str {
        match self {
            RestingKind::Swap(_) => "swap",
            RestingKind::Perp(_) => "perp",
        }
    }
}

/// A limit order resting in the book, awaiting a touch or expiry.
struct RestingOrder {
    id: String,
    node_id: String,
    /// Tick index at placement; the order is only eligible from the next tick on.
    placed_index: usize,
    /// `Some(n)` = good-til-`n`-bars; `None` = good-til-cancelled.
    expire_after_bars: Option<u32>,
    placed: PlacedLimit,
    kind: RestingKind,
    /// Action ids to chain when (and only when) the order fills.
    downstream: Vec<String>,
}

/// What happened when the engine attempted an action node.
enum ActionOutcome {
    Executed(Fill),
    Rejected(String),
    Resting(RestingSpec),
}

/// A validated limit placement ready to enter the resting book.
struct RestingSpec {
    placed: PlacedLimit,
    kind: RestingKind,
    expire_after_bars: Option<u32>,
}

impl From<Execution> for ActionOutcome {
    fn from(e: Execution) -> Self {
        match e {
            Execution::Executed(fill) => ActionOutcome::Executed(fill),
            Execution::Rejected { reason } => ActionOutcome::Rejected(reason),
        }
    }
}

fn is_limit(order_type: &str) -> bool {
    order_type == "limit"
}

/// Resolve time-in-force to an optional bar count. `gtc` (or absent) never expires.
fn resolve_expiry(time_in_force: &Option<String>, expire_after_bars: Option<u32>) -> Option<u32> {
    match time_in_force.as_deref() {
        Some("gtc") => None,
        _ => expire_after_bars,
    }
}

/// Count interior missing buckets in a sorted timestamp series given the step.
fn interior_missing(ts_sorted: &[i64], step: i64) -> usize {
    let mut missing = 0usize;
    for w in ts_sorted.windows(2) {
        if w[1] - w[0] > step {
            missing += (w[1] - w[0]) as usize / step as usize - 1;
        }
    }
    missing
}

/// Check required candle series for interior gaps within the window. Under
/// `missing_required = fail` a gap aborts the run; otherwise it's a warning.
fn check_required_coverage(
    compiled: &catalyst_graph_compiler::CompiledGraph,
    index: &BundleIndex,
    start: i64,
    end: i64,
    step: i64,
    policy: &ResolvedPolicy,
    warnings: &mut Vec<String>,
) -> Result<(), EngineError> {
    if step <= 0 {
        return Ok(());
    }
    for req in &compiled.data_requirements.candles {
        let ts = index.candle_ts_in(&req.venue, &req.symbol, start, end);
        if ts.len() < 2 {
            continue; // leading/trailing absence isn't an interior hole
        }
        let missing = interior_missing(&ts, step);
        if missing > 0 {
            let msg = format!(
                "required candles {}/{} have {missing} missing bar(s) inside the window",
                req.venue, req.symbol
            );
            if policy.missing_required == MissingRequired::Fail {
                return Err(EngineError::Data(msg));
            }
            warnings.push(msg);
        }
    }
    Ok(())
}

fn interval_seconds(interval: &str) -> Option<i64> {
    Some(match interval {
        "1m" => 60,
        "5m" => 300,
        "15m" => 900,
        "1h" => 3600,
        "4h" => 14_400,
        "1d" => 86_400,
        _ => return None,
    })
}

/// Run a full simulation, returning a deterministic trace.
pub fn run(input: &SimulationInput) -> Result<SimulationTrace, EngineError> {
    let mut policy = resolve_policy(&input.policy).map_err(|e| EngineError::Policy(e.to_string()))?;
    // Per-run execution overrides (config.execution) win over the profile.
    if let Some(overrides) = &input.config.execution {
        policy
            .apply_execution_overrides(overrides)
            .map_err(|e| EngineError::Policy(e.to_string()))?;
    }
    let interval = &input.config.interval;
    let interval_secs = interval_seconds(interval)
        .ok_or_else(|| EngineError::Config(format!("unknown interval {interval:?}")))?;
    let start = parse_ts(&input.config.start)
        .ok_or_else(|| EngineError::Config("bad start timestamp".into()))?;
    let end =
        parse_ts(&input.config.end).ok_or_else(|| EngineError::Config("bad end timestamp".into()))?;

    let index = BundleIndex::build(&input.market_data);
    let compiled = catalyst_graph_compiler::compile(&input.graph)
        .map_err(|e| EngineError::Config(format!("graph did not compile: {e}")))?;
    let exec_graph = ExecGraph::from_compiled(&compiled);

    let allow_negative = policy.insufficient_balance == InsufficientBalance::AllowNegative;
    let mut ledger = initial_ledger(&input.config, allow_negative);

    let mut events: Vec<Event> = Vec::new();
    let mut snapshots: Vec<Snapshot> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut signal_state: HashMap<String, bool> = HashMap::new();
    let mut signals_ever_fired: HashSet<String> = HashSet::new();
    // Resting limit orders awaiting a touch, plus a monotonic id counter.
    let mut resting: Vec<RestingOrder> = Vec::new();
    let mut order_seq: u64 = 0;
    let mut signal_last_fired: HashMap<String, i64> = HashMap::new();
    let mut signal_fire_count: HashMap<String, u32> = HashMap::new();
    let variables = parse_variables(&input.graph.variables);

    // Intra-window gap check on required candle series (#42): a hole inside a
    // required series fails the run under `missing_required = fail`, else warns.
    check_required_coverage(&compiled, &index, start, end, interval_secs, &policy, &mut warnings)?;

    let mut ticks = index.ticks(start, end);
    if !exec_graph.initial_actions.is_empty() && !ticks.contains(&start) {
        ticks.push(start);
        ticks.sort_unstable();
        ticks.dedup();
    }
    if ticks.is_empty() {
        ticks.push(start);
        warnings.push("no market data in range; ran a single degenerate tick".into());
    }
    let mut initial_done = false;
    let mut last_ts_iso = input.config.end.clone();
    // The tick clock is data-driven and may be gapped or coarser than the configured
    // interval, so accrual is scaled by the *actual* seconds since the previous tick
    // rather than a fixed `interval_secs`. On the first tick there's no prior tick
    // (and no positions yet), so the configured interval is a harmless default.
    let mut prev_ts: Option<i64> = None;

    for (tick_index, ts) in ticks.into_iter().enumerate() {
        let ts_iso = format_ts(ts);
        last_ts_iso = ts_iso.clone();
        let elapsed_secs = prev_ts.map(|p| ts - p).unwrap_or(interval_secs);
        prev_ts = Some(ts);

        accrue_funding(&mut ledger, &index, ts, elapsed_secs, &ts_iso, &policy, &mut events);
        accrue_yield(&mut ledger, &index, ts, elapsed_secs, &ts_iso, &mut events);
        check_liquidations(&mut ledger, &index, ts, &ts_iso, &policy, &mut events);

        // Tick-start equity, used to resolve pct_portfolio sizing for any action
        // this tick (snapshots below recompute it post-action).
        let tick_equity = compute_equity(&ledger, &index, ts);

        // Resting orders placed on earlier bars get a chance to fill/expire first.
        fill_resting_orders(
            tick_index, ts, &ts_iso, &exec_graph, &mut ledger, &index, &policy, tick_equity,
            &mut events, &mut resting, &mut order_seq,
        );

        let ctx = TickContext { index: &index, ts };

        if !initial_done {
            for action_id in &exec_graph.initial_actions {
                run_action_chain(
                    action_id, &exec_graph, &mut ledger, &ctx, &policy, &ts_iso, tick_index,
                    tick_equity, &mut events, &mut resting, &mut order_seq,
                );
            }
            initial_done = true;
        }

        evaluate_signals(
            &exec_graph,
            &mut ledger,
            &ctx,
            &index,
            ts,
            &ts_iso,
            tick_index,
            interval_secs,
            tick_equity,
            &policy,
            &mut events,
            &mut signal_state,
            &mut signals_ever_fired,
            &mut signal_last_fired,
            &mut signal_fire_count,
            &variables,
            &mut warnings,
            &mut resting,
            &mut order_seq,
        );

        let equity = compute_equity(&ledger, &index, ts);
        snapshots.push(Snapshot {
            ts: ts_iso.clone(),
            equity_usd: equity.normalize().to_string(),
            portfolio: Some(ledger.to_portfolio()),
        });
    }

    // Any orders still resting at the end expire unfilled.
    for order in resting.drain(..) {
        events.push(Event {
            ts: last_ts_iso.clone(),
            event_type: "order_expired".into(),
            node_id: Some(order.node_id),
            reason: Some("backtest ended with order resting".into()),
            detail: Some(json!({ "order_id": order.id })),
        });
    }

    Ok(SimulationTrace {
        schema_version: "catalyst.backtest.trace.v1".into(),
        policy: policy.to_contract(),
        interval: interval.clone(),
        start: input.config.start.clone(),
        end: input.config.end.clone(),
        snapshots,
        events,
        final_portfolio: ledger.to_portfolio(),
        warnings,
        errors: Vec::new(),
    })
}

fn initial_ledger(config: &BacktestConfig, allow_negative: bool) -> Ledger {
    let mut balances: BTreeMap<String, BTreeMap<String, Decimal>> = BTreeMap::new();
    for (venue, assets) in &config.initial_portfolio {
        let mut out = BTreeMap::new();
        for (asset, amount) in assets {
            out.insert(asset.clone(), amount.parse().unwrap_or(Decimal::ZERO));
        }
        balances.insert(venue.clone(), out);
    }
    Ledger::with_initial(balances, allow_negative)
}

#[allow(clippy::too_many_arguments)]
fn evaluate_signals(
    exec_graph: &ExecGraph,
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    index: &BundleIndex,
    ts: i64,
    ts_iso: &str,
    tick_index: usize,
    interval_secs: i64,
    equity: Decimal,
    policy: &ResolvedPolicy,
    events: &mut Vec<Event>,
    signal_state: &mut HashMap<String, bool>,
    ever_fired: &mut HashSet<String>,
    last_fired: &mut HashMap<String, i64>,
    fire_count: &mut HashMap<String, u32>,
    variables: &HashMap<String, Decimal>,
    warnings: &mut Vec<String>,
    resting: &mut Vec<RestingOrder>,
    order_seq: &mut u64,
) {
    // Phase 1: compute every signal's boolean condition this tick. Signals are
    // in topological order (compiler-emitted), so a combinator's inputs are
    // already resolved. `None` = a leaf had no data this tick.
    let mut conditions: HashMap<&str, Option<bool>> = HashMap::new();
    let mut leaf_values: HashMap<&str, (Decimal, Decimal)> = HashMap::new();
    for signal in &exec_graph.signals {
        let cond = match &signal.def {
            SignalDef::Threshold { source, operator, reference } => {
                match (
                    source_value(source, index, ts, interval_secs),
                    reference_value(reference, index, ts, interval_secs, variables),
                ) {
                    (Some(lhs), Some(rhs)) => {
                        leaf_values.insert(signal.id.as_str(), (lhs, rhs));
                        Some(eval_threshold(lhs, operator, rhs))
                    }
                    _ => {
                        warnings.push(format!("no data for signal {:?} at {ts_iso}", signal.id));
                        None
                    }
                }
            }
            SignalDef::Combinator { op, inputs } => {
                let read = |id: &str| conditions.get(id).copied().flatten().unwrap_or(false);
                let result = match op {
                    CombinatorOp::All => inputs.iter().all(|i| read(i)),
                    CombinatorOp::Any => inputs.iter().any(|i| read(i)),
                    CombinatorOp::Not => !inputs.first().map(|i| read(i)).unwrap_or(false),
                };
                Some(result)
            }
        };
        conditions.insert(signal.id.as_str(), cond);
    }

    // Phase 2: apply firing semantics to signals that drive actions.
    for signal in &exec_graph.signals {
        if signal.targets.is_empty() {
            continue;
        }
        // A leaf with no data this tick: skip without disturbing crossing state.
        let condition = match conditions.get(signal.id.as_str()).copied().flatten() {
            Some(c) => c,
            None => continue,
        };
        let previous = signal_state.get(&signal.id).copied().unwrap_or(false);
        signal_state.insert(signal.id.clone(), condition);

        // 1. Trigger edge: when does the condition "fire" at all?
        let edge = match policy.signal_trigger {
            SignalTrigger::Level | SignalTrigger::OncePerBacktest => condition,
            SignalTrigger::Crossing | SignalTrigger::CrossingWithCooldown => condition && !previous,
        };
        if !edge {
            continue;
        }

        // 2. once_per_backtest: only the first fire, ever.
        if policy.signal_trigger == SignalTrigger::OncePerBacktest
            && ever_fired.contains(&signal.id)
        {
            continue;
        }

        // 3. Repeat gate.
        let count = fire_count.get(&signal.id).copied().unwrap_or(0);
        let repeat_ok = match policy.repeat {
            Repeat::Never => count == 0,
            Repeat::OnEachSignalFire | Repeat::WithCooldown => true,
            Repeat::MaxCount => policy.repeat_max_count.map_or(count == 0, |m| count < m),
        };
        if !repeat_ok {
            continue;
        }

        // 4. Cooldown gate (crossing_with_cooldown trigger or with_cooldown repeat).
        let needs_cooldown = policy.signal_trigger == SignalTrigger::CrossingWithCooldown
            || policy.repeat == Repeat::WithCooldown;
        if needs_cooldown {
            if let (Some(cd), Some(&last)) = (
                policy.cooldown.as_deref().and_then(parse_duration_secs),
                last_fired.get(&signal.id),
            ) {
                if ts - last < cd {
                    continue;
                }
            }
        }

        // Fire.
        ever_fired.insert(signal.id.clone());
        *fire_count.entry(signal.id.clone()).or_insert(0) += 1;
        last_fired.insert(signal.id.clone(), ts);
        events.push(Event {
            ts: ts_iso.to_string(),
            event_type: "signal_fired".into(),
            node_id: Some(signal.id.clone()),
            reason: None,
            detail: Some(signal_detail(signal, &leaf_values, condition)),
        });
        for target in &signal.targets {
            run_action_chain(
                target, exec_graph, ledger, ctx, policy, ts_iso, tick_index, equity, events,
                resting, order_seq,
            );
        }
    }
}

/// The per-tick scalar a [`Source`] observes, from the indexed market data.
fn source_value(
    source: &Source,
    index: &BundleIndex,
    ts: i64,
    interval_secs: i64,
) -> Option<Decimal> {
    match source {
        Source::Price { symbol, venue } => match venue {
            Some(v) => index.bar_at(v, symbol, ts).map(|b| b.close),
            None => index.price_any(symbol, ts),
        },
        Source::Funding { venue, symbol } => index.funding_at(venue, symbol, ts),
        Source::Yield { protocol, asset, chain, pool } => {
            let key = (protocol.clone(), asset.clone(), chain.clone(), pool.clone());
            index.apr_at(&key, ts)
        }
        Source::Gas { chain } => index.gas_at(chain, ts),
        Source::Derived { of, transform, window } => {
            let w = (*window).max(1) as i64;
            // Sample the underlying source at the last `window` grid bars,
            // newest first; stop at the first gap.
            let mut samples: Vec<Decimal> = Vec::with_capacity(w as usize);
            for k in 0..w {
                match source_value(of, index, ts - k * interval_secs, interval_secs) {
                    Some(v) => samples.push(v),
                    None => break,
                }
            }
            // Require a full window of warmup history before the signal is valid.
            if (samples.len() as u32) < *window {
                return None;
            }
            Some(apply_transform(transform, &samples))
        }
    }
}

/// Apply a [`Transform`] to samples ordered newest-first.
fn apply_transform(transform: &Transform, samples: &[Decimal]) -> Decimal {
    match transform {
        Transform::Sma => {
            samples.iter().copied().sum::<Decimal>() / Decimal::from(samples.len() as u64)
        }
        Transform::RollingHigh => samples.iter().copied().max().unwrap_or(Decimal::ZERO),
        Transform::RollingLow => samples.iter().copied().min().unwrap_or(Decimal::ZERO),
        Transform::Roc => {
            let current = samples[0];
            let oldest = *samples.last().unwrap();
            if oldest.is_zero() {
                Decimal::ZERO
            } else {
                (current - oldest) / oldest
            }
        }
        Transform::Ema => {
            // Fold oldest -> newest with alpha = 2 / (n + 1).
            let alpha = Decimal::from(2) / Decimal::from(samples.len() as u64 + 1);
            let mut iter = samples.iter().rev();
            let mut ema = *iter.next().unwrap();
            for &v in iter {
                ema = alpha * v + (Decimal::ONE - alpha) * ema;
            }
            ema
        }
    }
}

/// The right-hand side of a signal comparison.
fn reference_value(
    reference: &Reference,
    index: &BundleIndex,
    ts: i64,
    interval_secs: i64,
    variables: &HashMap<String, Decimal>,
) -> Option<Decimal> {
    match reference {
        Reference::Const { value } => value.parse::<Decimal>().ok(),
        Reference::Source { source } => source_value(source, index, ts, interval_secs),
        Reference::Var { var } => variables.get(var).copied(),
    }
}

/// Parse a duration like `30s`, `15m`, `1h`, `2d` into seconds.
fn parse_duration_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: i64 = num.parse().ok()?;
    let mult = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return None,
    };
    Some(n * mult)
}

/// Parse `Graph.variables` (a JSON object of name -> decimal-ish value) into a
/// lookup the engine can resolve `Reference::Var` against.
fn parse_variables(value: &serde_json::Value) -> HashMap<String, Decimal> {
    let mut out = HashMap::new();
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            let parsed = match v {
                serde_json::Value::String(s) => s.parse::<Decimal>().ok(),
                serde_json::Value::Number(n) => n.to_string().parse::<Decimal>().ok(),
                _ => None,
            };
            if let Some(d) = parsed {
                out.insert(k.clone(), d);
            }
        }
    }
    out
}

/// Build the `signal_fired` event detail. Threshold (leaf) signals keep the
/// legacy `symbol`/`price` keys for price sources and also report the generic
/// `source`/`value`; combinators report their op, inputs, and result.
fn signal_detail(
    signal: &Signal,
    leaf_values: &HashMap<&str, (Decimal, Decimal)>,
    condition: bool,
) -> serde_json::Value {
    let mut d = serde_json::Map::new();
    match &signal.def {
        SignalDef::Threshold { source, operator, .. } => {
            let (lhs, rhs) =
                leaf_values.get(signal.id.as_str()).copied().unwrap_or((Decimal::ZERO, Decimal::ZERO));
            d.insert("operator".into(), json!(operator));
            d.insert("value".into(), json!(lhs.normalize().to_string()));
            d.insert("threshold".into(), json!(rhs.normalize().to_string()));
            d.insert("source".into(), serde_json::to_value(source).unwrap_or(serde_json::Value::Null));
            if let Source::Price { symbol, .. } = source {
                d.insert("symbol".into(), json!(symbol));
                d.insert("price".into(), json!(lhs.normalize().to_string()));
            }
        }
        SignalDef::Combinator { op, inputs } => {
            let op_str = match op {
                CombinatorOp::All => "all",
                CombinatorOp::Any => "any",
                CombinatorOp::Not => "not",
            };
            d.insert("op".into(), json!(op_str));
            d.insert("inputs".into(), json!(inputs));
            d.insert("result".into(), json!(condition));
        }
    }
    serde_json::Value::Object(d)
}

#[allow(clippy::too_many_arguments)]
fn run_action_chain(
    action_id: &str,
    exec_graph: &ExecGraph,
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    ts_iso: &str,
    tick_index: usize,
    equity: Decimal,
    events: &mut Vec<Event>,
    resting: &mut Vec<RestingOrder>,
    order_seq: &mut u64,
) {
    let mut stack = vec![action_id.to_string()];
    let mut visited: HashSet<String> = HashSet::new();
    while let Some(id) = stack.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(action) = exec_graph.actions.get(&id) else { continue };

        // Execute on a private copy of the ledger and only commit it if the action
        // fully filled (compare-and-swap style). This makes every action atomic:
        // a rejection — including a partway failure in a multi-step model such as a
        // yield deposit whose gas can't be covered — leaves the real ledger
        // untouched, so individual models don't need to hand-roll rollbacks.
        let mut trial = ledger.clone();
        match execute_action(action, &mut trial, ctx, policy, equity) {
            ActionOutcome::Executed(fill) => {
                *ledger = trial; // commit the trial copy
                events.push(Event {
                    ts: ts_iso.to_string(),
                    event_type: "action_executed".into(),
                    node_id: Some(id.clone()),
                    reason: None,
                    detail: serde_json::to_value(&fill).ok(),
                });
                if let Some(downstream) = exec_graph.out_action_edges.get(&id) {
                    for next in downstream {
                        stack.push(next.clone());
                    }
                }
            }
            ActionOutcome::Rejected(reason) => {
                events.push(Event {
                    ts: ts_iso.to_string(),
                    event_type: "action_rejected".into(),
                    node_id: Some(id.clone()),
                    reason: Some(reason),
                    detail: None,
                });
            }
            ActionOutcome::Resting(spec) => {
                // Placement reads the ledger but never mutates it; downstream
                // actions are deferred until (and unless) the order fills.
                let downstream =
                    exec_graph.out_action_edges.get(&id).cloned().unwrap_or_default();
                let order_id = format!("{id}#{}", *order_seq);
                *order_seq += 1;
                events.push(Event {
                    ts: ts_iso.to_string(),
                    event_type: "order_placed".into(),
                    node_id: Some(id.clone()),
                    reason: None,
                    detail: Some(json!({
                        "order_id": order_id,
                        "kind": spec.kind.label(),
                        "side": spec.placed.side.as_str(),
                        "limit_price": spec.placed.limit.normalize().to_string(),
                        "venue": spec.placed.venue,
                        "symbol": spec.placed.symbol,
                        "expire_after_bars": spec.expire_after_bars,
                    })),
                });
                resting.push(RestingOrder {
                    id: order_id,
                    node_id: id.clone(),
                    placed_index: tick_index,
                    expire_after_bars: spec.expire_after_bars,
                    placed: spec.placed,
                    kind: spec.kind,
                    downstream,
                });
            }
        }
    }
}

/// Scan resting orders against the current bar: fill the touched ones (running
/// their downstream chains), expire any past their time-in-force, and keep the
/// rest. Orders are only eligible from the bar *after* they were placed, so we
/// never use intra-placement-bar information we couldn't have known.
#[allow(clippy::too_many_arguments)]
fn fill_resting_orders(
    tick_index: usize,
    ts: i64,
    ts_iso: &str,
    exec_graph: &ExecGraph,
    ledger: &mut Ledger,
    index: &BundleIndex,
    policy: &ResolvedPolicy,
    equity: Decimal,
    events: &mut Vec<Event>,
    resting: &mut Vec<RestingOrder>,
    order_seq: &mut u64,
) {
    let ctx = TickContext { index, ts };
    let ready: Vec<RestingOrder> = std::mem::take(resting);
    let mut keep: Vec<RestingOrder> = Vec::new();

    for order in ready {
        // Next-bar eligibility: an order placed at tick T is checked from T+1 on.
        if order.placed_index >= tick_index {
            keep.push(order);
            continue;
        }
        // Time-in-force: expire before attempting a fill on a too-late bar.
        if let Some(n) = order.expire_after_bars {
            if tick_index > order.placed_index + n as usize {
                events.push(Event {
                    ts: ts_iso.to_string(),
                    event_type: "order_expired".into(),
                    node_id: Some(order.node_id.clone()),
                    reason: Some("time_in_force elapsed".into()),
                    detail: Some(json!({ "order_id": order.id })),
                });
                continue;
            }
        }
        let bar = match ctx.bar(&order.placed.venue, &order.placed.symbol) {
            Some(b) => b,
            None => {
                keep.push(order); // no data this bar; try again later
                continue;
            }
        };
        let Some(price) = limit_fill_price(&bar, order.placed.side, order.placed.limit) else {
            keep.push(order);
            continue;
        };

        // Fill atomically at the touched price (maker — no taker slippage).
        let mut trial = ledger.clone();
        let outcome = match &order.kind {
            RestingKind::Swap(cfg) => execute_swap_at(&mut trial, &ctx, policy, cfg, price),
            RestingKind::Perp(cfg) => execute_perp_at(&mut trial, policy, cfg, price),
        };
        match outcome {
            Execution::Executed(fill) => {
                *ledger = trial;
                let mut detail = serde_json::to_value(&fill).unwrap_or(Value::Null);
                if let Value::Object(map) = &mut detail {
                    map.insert("order_id".into(), json!(order.id));
                    map.insert(
                        "limit_price".into(),
                        json!(order.placed.limit.normalize().to_string()),
                    );
                }
                events.push(Event {
                    ts: ts_iso.to_string(),
                    event_type: "order_filled".into(),
                    node_id: Some(order.node_id.clone()),
                    reason: None,
                    detail: Some(detail),
                });
                // The order's downstream chain runs now, at the fill bar.
                for target in &order.downstream {
                    run_action_chain(
                        target, exec_graph, ledger, &ctx, policy, ts_iso, tick_index, equity,
                        events, resting, order_seq,
                    );
                }
            }
            Execution::Rejected { reason } => {
                events.push(Event {
                    ts: ts_iso.to_string(),
                    event_type: "order_rejected".into(),
                    node_id: Some(order.node_id.clone()),
                    reason: Some(reason),
                    detail: Some(json!({ "order_id": order.id })),
                });
            }
        }
    }

    // Orders placed by downstream chains this tick are already in `resting`;
    // append the ones that are still waiting.
    resting.extend(keep);
}

fn execute_action(
    action: &ActionNode,
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    equity: Decimal,
) -> ActionOutcome {
    match action.subtype.as_str() {
        "swap" => match serde_json::from_value::<SwapConfig>(action.config.clone()) {
            Ok(mut cfg) => {
                // A swap has no distinct "position"; both balance/position bases
                // resolve against the from-asset balance. pct_portfolio converts
                // the USD slice back into from-asset units via its price.
                let bal = ledger.balance(&cfg.chain, &cfg.from_asset);
                let unit_price = asset_price(ctx, &cfg.chain, &cfg.from_asset);
                match resolve_amount(&cfg.amount, bal, bal, equity, unit_price) {
                    Ok(a) => cfg.amount = a,
                    Err(e) => return ActionOutcome::Rejected(e),
                }
                if is_limit(&cfg.order_type) {
                    match place_swap_limit(&cfg) {
                        LimitPlacement::Placed(placed) => ActionOutcome::Resting(RestingSpec {
                            placed,
                            expire_after_bars: resolve_expiry(
                                &cfg.time_in_force,
                                cfg.expire_after_bars,
                            ),
                            kind: RestingKind::Swap(cfg),
                        }),
                        LimitPlacement::Rejected(e) => ActionOutcome::Rejected(e),
                    }
                } else {
                    execute_swap(ledger, ctx, policy, &cfg).into()
                }
            }
            Err(e) => ActionOutcome::Rejected(format!("bad swap config: {e}")),
        },
        "perp_order" => match serde_json::from_value::<PerpOrderConfig>(action.config.clone()) {
            Ok(mut cfg) => {
                let bal = ledger.balance(&cfg.chain, "USDC");
                let position = ledger
                    .perp(&cfg.chain, &cfg.symbol)
                    .map(|p| (p.size * p.entry_price).abs())
                    .unwrap_or(Decimal::ZERO);
                // size_usd is already USD, so pct_portfolio needs no conversion.
                match resolve_amount(&cfg.size_usd, bal, position, equity, Decimal::ONE) {
                    Ok(a) => cfg.size_usd = a,
                    Err(e) => return ActionOutcome::Rejected(e),
                }
                if is_limit(&cfg.order_type) {
                    match place_perp_limit(ledger, &cfg) {
                        LimitPlacement::Placed(placed) => ActionOutcome::Resting(RestingSpec {
                            placed,
                            expire_after_bars: resolve_expiry(
                                &cfg.time_in_force,
                                cfg.expire_after_bars,
                            ),
                            kind: RestingKind::Perp(cfg),
                        }),
                        LimitPlacement::Rejected(e) => ActionOutcome::Rejected(e),
                    }
                } else {
                    execute_perp(ledger, ctx, policy, &cfg).into()
                }
            }
            Err(e) => ActionOutcome::Rejected(format!("bad perp config: {e}")),
        },
        "yield_deposit" => match serde_json::from_value::<YieldConfig>(action.config.clone()) {
            Ok(mut cfg) => match resolve_yield_amount(&mut cfg, ledger, ctx, equity) {
                Ok(()) => execute_yield_deposit(ledger, ctx, policy, &cfg).into(),
                Err(e) => ActionOutcome::Rejected(e),
            },
            Err(e) => ActionOutcome::Rejected(format!("bad yield config: {e}")),
        },
        "yield_withdraw" => match serde_json::from_value::<YieldConfig>(action.config.clone()) {
            Ok(mut cfg) => match resolve_yield_amount(&mut cfg, ledger, ctx, equity) {
                Ok(()) => execute_yield_withdraw(ledger, ctx, policy, &cfg).into(),
                Err(e) => ActionOutcome::Rejected(e),
            },
            Err(e) => ActionOutcome::Rejected(format!("bad yield config: {e}")),
        },
        other => ActionOutcome::rejected_subtype(other),
    }
}

/// Mark price of `asset` on `venue` for unit conversion (1 for stables).
fn asset_price(ctx: &dyn MarketContext, venue: &str, asset: &str) -> Decimal {
    if is_stable(asset) {
        Decimal::ONE
    } else {
        ctx.bar(venue, asset).map(|b| b.close).unwrap_or(Decimal::ZERO)
    }
}

/// Resolve a relative [`Amount`] to an absolute decimal string against the
/// supplied bases. `pct_portfolio` is a USD slice of total equity; for
/// unit-denominated actions (swap/yield) it is converted to asset units via
/// `unit_price` (1 for USD-denominated perp size).
fn resolve_amount(
    amount: &Amount,
    balance: Decimal,
    position: Decimal,
    equity: Decimal,
    unit_price: Decimal,
) -> Result<Amount, String> {
    match amount {
        Amount::Absolute(_) => Ok(amount.clone()),
        Amount::Relative { basis, value } => {
            let pct = value.parse::<Decimal>().unwrap_or(Decimal::ZERO) / Decimal::from(100);
            let resolved = match basis {
                AmountBasis::PctBalance => pct * balance,
                AmountBasis::PctPosition => pct * position,
                AmountBasis::PctPortfolio => {
                    if unit_price.is_zero() {
                        return Err(
                            "pct_portfolio sizing needs a price for the action asset".to_string()
                        );
                    }
                    pct * equity / unit_price
                }
            };
            Ok(Amount::Absolute(resolved.normalize().to_string()))
        }
    }
}

fn resolve_yield_amount(
    cfg: &mut YieldConfig,
    ledger: &Ledger,
    ctx: &dyn MarketContext,
    equity: Decimal,
) -> Result<(), String> {
    let balance = ledger.balance(&cfg.chain, &cfg.asset);
    let position = ledger
        .yields()
        .find(|y| {
            y.protocol == cfg.protocol
                && y.asset == cfg.asset
                && y.chain == cfg.chain
                && y.pool == cfg.pool
        })
        .map(|y| y.principal + y.accrued)
        .unwrap_or(Decimal::ZERO);
    let unit_price = asset_price(ctx, &cfg.chain, &cfg.asset);
    cfg.amount = resolve_amount(&cfg.amount, balance, position, equity, unit_price)?;
    Ok(())
}

impl ActionOutcome {
    fn rejected_subtype(subtype: &str) -> Self {
        ActionOutcome::Rejected(format!("unsupported action subtype {subtype}"))
    }
}

fn mark_price(index: &BundleIndex, venue: &str, symbol: &str, ts: i64) -> Option<Decimal> {
    // Venue-scoped close at-or-before `ts` (#119): a position is valued from its
    // OWN venue's candles, never another venue's that happens to share the symbol.
    index.close_at(venue, symbol, ts)
}

fn accrue_funding(
    ledger: &mut Ledger,
    index: &BundleIndex,
    ts: i64,
    elapsed_secs: i64,
    ts_iso: &str,
    policy: &ResolvedPolicy,
    events: &mut Vec<Event>,
) {
    if policy.funding != Funding::Historical {
        return;
    }
    let perps: Vec<PerpPosition> = ledger.perps().cloned().collect();
    for p in perps {
        // Sum every funding point since the previous tick, `(ts - elapsed, ts]`,
        // not just the one at `ts` — so a tick interval coarser than the funding
        // interval (e.g. 4h ticks with hourly funding) accrues all of it rather
        // than 1/N, and a gapped tick clock covers the whole elapsed window.
        let rate = index.funding_sum(&p.venue, &p.symbol, ts - elapsed_secs, ts);
        if rate.is_zero() {
            continue;
        }
        let Some(mark) = mark_price(index, &p.venue, &p.symbol, ts) else { continue };
        let notional = p.size * mark;
        let sign = match p.side {
            PerpSide::Long => Decimal::ONE,
            PerpSide::Short => Decimal::NEGATIVE_ONE,
        };
        let payment = sign * rate * notional; // positive = we pay
        if payment.is_zero() {
            continue;
        }
        ledger.credit(&p.venue, "USDC", -payment);
        ledger.record_funding(payment);
        events.push(Event {
            ts: ts_iso.to_string(),
            event_type: "funding_applied".into(),
            node_id: None,
            reason: None,
            detail: Some(json!({
                "venue": p.venue,
                "symbol": p.symbol,
                "rate": rate.normalize().to_string(),
                "payment_usd": payment.normalize().to_string(),
            })),
        });
    }
}

fn accrue_yield(
    ledger: &mut Ledger,
    index: &BundleIndex,
    ts: i64,
    elapsed_secs: i64,
    ts_iso: &str,
    events: &mut Vec<Event>,
) {
    let positions: Vec<YieldPosition> = ledger.yields().cloned().collect();
    let fraction = Decimal::from(elapsed_secs) / Decimal::from(YEAR_SECONDS);
    for y in positions {
        let key = (y.protocol.clone(), y.asset.clone(), y.chain.clone(), y.pool.clone());
        let Some(apr) = index.apr_at(&key, ts) else { continue };
        let interest = y.principal * apr * fraction;
        if interest.is_zero() {
            continue;
        }
        let _ = ledger.accrue_yield(&y.protocol, &y.asset, &y.chain, y.pool.as_deref(), interest);
        events.push(Event {
            ts: ts_iso.to_string(),
            event_type: "yield_accrued".into(),
            node_id: None,
            reason: None,
            detail: Some(json!({
                "protocol": y.protocol,
                "asset": y.asset,
                "chain": y.chain,
                "apr": apr.normalize().to_string(),
                "interest_usd": interest.normalize().to_string(),
            })),
        });
    }
}

fn check_liquidations(
    ledger: &mut Ledger,
    index: &BundleIndex,
    ts: i64,
    ts_iso: &str,
    policy: &ResolvedPolicy,
    events: &mut Vec<Event>,
) {
    if policy.liquidation_check != LiquidationCheck::EveryTick {
        return;
    }
    let perps: Vec<PerpPosition> = ledger.perps().cloned().collect();
    for p in perps {
        let Some(mark) = mark_price(index, &p.venue, &p.symbol, ts) else { continue };
        if p.unrealized_pnl(mark) <= -p.margin_usd {
            // Liquidation: margin is lost, position removed (settle nothing back).
            let _ = ledger.close_perp(&p.venue, &p.symbol, Decimal::ZERO);
            events.push(Event {
                ts: ts_iso.to_string(),
                event_type: "liquidation".into(),
                node_id: None,
                reason: Some(format!("{} {} position liquidated", p.venue, p.symbol)),
                detail: Some(json!({
                    "venue": p.venue,
                    "symbol": p.symbol,
                    "mark": mark.normalize().to_string(),
                    "margin_lost_usd": p.margin_usd.normalize().to_string(),
                })),
            });
        }
    }
}

/// Mark-to-market portfolio value in USD.
fn compute_equity(ledger: &Ledger, index: &BundleIndex, ts: i64) -> Decimal {
    let portfolio: Portfolio = ledger.to_portfolio();
    let mut equity = Decimal::ZERO;

    for (venue, assets) in &portfolio.balances {
        for (asset, amount) in assets {
            let amt: Decimal = amount.parse().unwrap_or(Decimal::ZERO);
            if is_stable(asset) {
                equity += amt;
            } else if let Some(price) = mark_price(index, venue, asset, ts) {
                equity += amt * price;
            }
        }
    }
    for p in ledger.perps() {
        if let Some(mark) = mark_price(index, &p.venue, &p.symbol, ts) {
            equity += p.margin_usd + p.unrealized_pnl(mark);
        } else {
            equity += p.margin_usd;
        }
    }
    for y in ledger.yields() {
        equity += y.value();
    }
    equity
}
