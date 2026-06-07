//! Run a few of the shipped example strategy graphs end-to-end over synthetic
//! market data, proving the new action/signal surface actually executes.

use std::collections::BTreeMap;
use std::path::PathBuf;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const START_EPOCH: i64 = 1_704_067_200;
const STEP: i64 = 3600;

fn ts(i: i64) -> String {
    let epoch = START_EPOCH + i * STEP;
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn example(name: &str) -> Graph {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/examples")
        .join(name)
        .canonicalize()
        .unwrap();
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn config(venue: &str, usdc: &str, n_ticks: i64) -> BacktestConfig {
    let mut bals = BTreeMap::new();
    bals.insert("USDC".to_string(), usdc.to_string());
    let mut initial = BTreeMap::new();
    initial.insert(venue.to_string(), bals);
    BacktestConfig {
        start: START.to_string(),
        end: ts(n_ticks),
        interval: "1h".to_string(),
        initial_portfolio: initial,
        execution: None,
    }
}

fn policy(trigger: &str) -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "signals": {"trigger": trigger}
    }))
    .unwrap()
}

fn candles(venue: &str, closes: &[&str]) -> Value {
    let points: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    json!([{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}])
}

fn gas(venue: &str, n: usize) -> Value {
    let pts: Vec<_> = (0..n).map(|i| json!({"ts": ts(i as i64), "gas_usd": "0.0"})).collect();
    json!([{"chain": venue, "points": pts}])
}

fn bundle(candles: Value, funding: Value, yields: Value, gas: Value, n: usize) -> MarketDataBundle {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(n as i64),
        "candles": candles, "funding": funding, "yields": yields, "gas": gas,
        "providers": [], "warnings": []
    }))
    .unwrap()
}

fn count(trace: &catalyst_contracts::SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

#[test]
fn funding_carry_opens_both_legs_when_funding_is_rich() {
    let venue = "hyperliquid";
    // rich only on the first bar, so the level signal fires exactly once
    let funding = json!([{"venue": venue, "symbol": "ETH",
        "points": [{"ts": ts(0), "rate": "0.0003"}, {"ts": ts(1), "rate": "0.00005"}]}]);
    let md = bundle(candles(venue, &["2000", "2000"]), funding, json!([]), gas(venue, 2), 2);
    let input = SimulationInput {
        graph: example("graph.funding-carry.json"),
        config: config(venue, "1000", 2),
        policy: policy("level"),
        market_data: md,
    };
    let trace = run(&input).unwrap();
    // one signal fans out to the long-spot and short-perp legs
    assert!(count(&trace, "signal_fired") >= 1);
    assert_eq!(count(&trace, "action_executed"), 2);
    assert_eq!(count(&trace, "action_rejected"), 0);
}

#[test]
fn stop_loss_exits_when_price_breaks_the_variable_level() {
    // buy at 2000, then price falls through the stop_price variable (1700).
    let md = bundle(candles("base", &["2000", "1600"]), json!([]), json!([]), gas("base", 2), 2);
    let input = SimulationInput {
        graph: example("graph.stop-loss.json"),
        config: config("base", "1000", 2),
        policy: policy("crossing"),
        market_data: md,
    };
    let trace = run(&input).unwrap();
    // initial open + the stop-out swap both execute
    assert_eq!(count(&trace, "action_executed"), 2);
    assert!(count(&trace, "signal_fired") >= 1);
    // ended in cash: no ETH left after the stop
    let eth = trace
        .final_portfolio
        .balances
        .get("base")
        .and_then(|b| b.get("ETH"))
        .map(|s| s.parse::<f64>().unwrap_or(0.0))
        .unwrap_or(0.0);
    assert!(eth.abs() < 1e-9, "expected flat ETH after stop, got {eth}");
}

#[test]
fn yield_rotation_deposits_when_apr_is_attractive() {
    let yields = json!([{"protocol": "aave", "asset": "USDC", "chain": "base", "pool": "usdc",
        "points": [{"ts": ts(0), "apr": "0.06"}, {"ts": ts(1), "apr": "0.06"}]}]);
    let md = bundle(candles("base", &["2000", "2000"]), json!([]), yields, gas("base", 2), 2);
    let input = SimulationInput {
        graph: example("graph.yield-rotation.json"),
        config: config("base", "1000", 2),
        policy: policy("level"),
        market_data: md,
    };
    let trace = run(&input).unwrap();
    assert!(count(&trace, "action_executed") >= 1);
    assert_eq!(count(&trace, "action_rejected"), 0);
}
