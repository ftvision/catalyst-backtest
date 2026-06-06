//! The deterministic tick/event loop.

use std::collections::{BTreeMap, HashMap, HashSet};

use rust_decimal::Decimal;
use serde_json::json;

use catalyst_contracts::trace::{Event, Portfolio, SimulationTrace, Snapshot};
use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_execution_models::{
    execute_perp, execute_swap, execute_yield_deposit, execute_yield_withdraw, is_stable, Execution,
    MarketContext,
};
use catalyst_portfolio_ledger::{Ledger, PerpPosition, PerpSide, YieldPosition};
use catalyst_simulation_policies::{
    resolve_policy, Funding, InsufficientBalance, LiquidationCheck, ResolvedPolicy, SignalTrigger,
};

use crate::exec_graph::{eval_threshold, ActionNode, ExecGraph};
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
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Policy(m) => write!(f, "policy error: {m}"),
            EngineError::Config(m) => write!(f, "config error: {m}"),
        }
    }
}
impl std::error::Error for EngineError {}

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
    let policy = resolve_policy(&input.policy).map_err(|e| EngineError::Policy(e.to_string()))?;
    let interval = &input.config.interval;
    let interval_secs = interval_seconds(interval)
        .ok_or_else(|| EngineError::Config(format!("unknown interval {interval:?}")))?;
    let start = parse_ts(&input.config.start)
        .ok_or_else(|| EngineError::Config("bad start timestamp".into()))?;
    let end =
        parse_ts(&input.config.end).ok_or_else(|| EngineError::Config("bad end timestamp".into()))?;

    let index = BundleIndex::build(&input.market_data);
    let exec_graph = ExecGraph::from_graph(&input.graph);

    let allow_negative = policy.insufficient_balance == InsufficientBalance::AllowNegative;
    let mut ledger = initial_ledger(&input.config, allow_negative);

    let mut events: Vec<Event> = Vec::new();
    let mut snapshots: Vec<Snapshot> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut signal_state: HashMap<String, bool> = HashMap::new();
    let mut signals_ever_fired: HashSet<String> = HashSet::new();

    let mut ticks = index.ticks(start, end);
    if ticks.is_empty() {
        ticks.push(start);
        warnings.push("no candle data in range; ran a single degenerate tick".into());
    }
    let mut initial_done = false;

    for ts in ticks {
        let ts_iso = format_ts(ts);

        accrue_funding(&mut ledger, &index, ts, &ts_iso, &policy, &mut events);
        accrue_yield(&mut ledger, &index, ts, interval_secs, &ts_iso, &mut events);
        check_liquidations(&mut ledger, &index, ts, &ts_iso, &policy, &mut events);

        let ctx = TickContext { index: &index, ts };

        if !initial_done {
            for action_id in &exec_graph.initial_actions {
                run_action_chain(action_id, &exec_graph, &mut ledger, &ctx, &policy, &ts_iso, &mut events);
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
            &policy,
            &mut events,
            &mut signal_state,
            &mut signals_ever_fired,
            &mut warnings,
        );

        let equity = compute_equity(&ledger, &index, ts);
        snapshots.push(Snapshot {
            ts: ts_iso.clone(),
            equity_usd: equity.normalize().to_string(),
            portfolio: Some(ledger.to_portfolio()),
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
    policy: &ResolvedPolicy,
    events: &mut Vec<Event>,
    signal_state: &mut HashMap<String, bool>,
    ever_fired: &mut HashSet<String>,
    warnings: &mut Vec<String>,
) {
    for signal in &exec_graph.signals {
        let price = match index.price_any(&signal.symbol, ts) {
            Some(p) => p,
            None => {
                warnings.push(format!("no price for signal symbol {} at {ts_iso}", signal.symbol));
                continue;
            }
        };
        let condition = eval_threshold(price, &signal.operator, signal.threshold);
        let previous = signal_state.get(&signal.id).copied().unwrap_or(false);

        let fired = match policy.signal_trigger {
            SignalTrigger::Level => condition,
            SignalTrigger::OncePerBacktest => condition && !ever_fired.contains(&signal.id),
            SignalTrigger::Crossing | SignalTrigger::CrossingWithCooldown => condition && !previous,
        };
        signal_state.insert(signal.id.clone(), condition);

        if fired {
            ever_fired.insert(signal.id.clone());
            events.push(Event {
                ts: ts_iso.to_string(),
                event_type: "signal_fired".into(),
                node_id: Some(signal.id.clone()),
                reason: None,
                detail: Some(json!({
                    "symbol": signal.symbol,
                    "operator": signal.operator,
                    "threshold": signal.threshold.normalize().to_string(),
                    "price": price.normalize().to_string(),
                })),
            });
            for target in &signal.targets {
                run_action_chain(target, exec_graph, ledger, ctx, policy, ts_iso, events);
            }
        }
    }
}

fn run_action_chain(
    action_id: &str,
    exec_graph: &ExecGraph,
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    ts_iso: &str,
    events: &mut Vec<Event>,
) {
    let mut stack = vec![action_id.to_string()];
    let mut visited: HashSet<String> = HashSet::new();
    while let Some(id) = stack.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(action) = exec_graph.actions.get(&id) else { continue };
        let executed = execute_and_log(action, ledger, ctx, policy, ts_iso, events);
        if executed {
            if let Some(downstream) = exec_graph.out_action_edges.get(&id) {
                for next in downstream {
                    stack.push(next.clone());
                }
            }
        }
    }
}

fn execute_and_log(
    action: &ActionNode,
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    ts_iso: &str,
    events: &mut Vec<Event>,
) -> bool {
    // Execute on a private copy of the ledger and only commit it if the action
    // fully filled (compare-and-swap style). This makes every action atomic:
    // a rejection — including a partway failure in a multi-step model such as a
    // yield deposit whose gas can't be covered — leaves the real ledger
    // untouched, so individual models don't need to hand-roll rollbacks.
    let mut trial = ledger.clone();
    let outcome = execute_action(action, &mut trial, ctx, policy);
    match outcome {
        Execution::Executed(fill) => {
            *ledger = trial; // commit the trial copy

            events.push(Event {
                ts: ts_iso.to_string(),
                event_type: "action_executed".into(),
                node_id: Some(action.id.clone()),
                reason: None,
                detail: serde_json::to_value(&fill).ok(),
            });
            true
        }
        Execution::Rejected { reason } => {
            events.push(Event {
                ts: ts_iso.to_string(),
                event_type: "action_rejected".into(),
                node_id: Some(action.id.clone()),
                reason: Some(reason),
                detail: None,
            });
            false
        }
    }
}

fn execute_action(
    action: &ActionNode,
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
) -> Execution {
    match action.subtype.as_str() {
        "swap" => match serde_json::from_value(action.config.clone()) {
            Ok(cfg) => execute_swap(ledger, ctx, policy, &cfg),
            Err(e) => Execution::rejected(format!("bad swap config: {e}")),
        },
        "perp_order" => match serde_json::from_value(action.config.clone()) {
            Ok(cfg) => execute_perp(ledger, ctx, policy, &cfg),
            Err(e) => Execution::rejected(format!("bad perp config: {e}")),
        },
        "yield_deposit" => match serde_json::from_value(action.config.clone()) {
            Ok(cfg) => execute_yield_deposit(ledger, ctx, policy, &cfg),
            Err(e) => Execution::rejected(format!("bad yield config: {e}")),
        },
        "yield_withdraw" => match serde_json::from_value(action.config.clone()) {
            Ok(cfg) => execute_yield_withdraw(ledger, ctx, policy, &cfg),
            Err(e) => Execution::rejected(format!("bad yield config: {e}")),
        },
        other => Execution::rejected(format!("unsupported action subtype {other}")),
    }
}

fn mark_price(index: &BundleIndex, venue: &str, symbol: &str, ts: i64) -> Option<Decimal> {
    index.bar_at(venue, symbol, ts).map(|b| b.close).or_else(|| index.price_any(symbol, ts))
}

fn accrue_funding(
    ledger: &mut Ledger,
    index: &BundleIndex,
    ts: i64,
    ts_iso: &str,
    policy: &ResolvedPolicy,
    events: &mut Vec<Event>,
) {
    if policy.funding != Funding::Historical {
        return;
    }
    let perps: Vec<PerpPosition> = ledger.perps().cloned().collect();
    for p in perps {
        let Some(rate) = index.funding_at(&p.venue, &p.symbol, ts) else { continue };
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
    interval_secs: i64,
    ts_iso: &str,
    events: &mut Vec<Event>,
) {
    let positions: Vec<YieldPosition> = ledger.yields().cloned().collect();
    let fraction = Decimal::from(interval_secs) / Decimal::from(YEAR_SECONDS);
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
