//! Behavioral tests for the ADR-0002 catalog strategies (strategies/graphs/
//! g19-g26). The catalog smoke test (`strategy_repository`) only checks they run
//! without errors over short scenarios; these drive synthetic data that makes
//! each strategy actually fire and execute.

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

fn graph(file: &str) -> Graph {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../strategies/graphs")
        .join(file)
        .canonicalize()
        .unwrap();
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn config(venue: &str, usdc: &str, n: i64) -> BacktestConfig {
    let mut bals = BTreeMap::new();
    bals.insert("USDC".to_string(), usdc.to_string());
    let mut initial = BTreeMap::new();
    initial.insert(venue.to_string(), bals);
    BacktestConfig {
        start: START.to_string(),
        // Last bar is ts(n - 1); #167 enforces the window matches the data.
        end: ts(n - 1),
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

fn bundle(venue: &str, closes: &[&str], funding: Value, yields: Value) -> MarketDataBundle {
    let points: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    let gas: Vec<_> =
        closes.iter().enumerate().map(|(i, _)| json!({"ts": ts(i as i64), "gas_usd": "0.0"})).collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(closes.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": funding, "yields": yields, "gas": [{"chain": venue, "points": gas}],
        "providers": [], "warnings": []
    }))
    .unwrap()
}

fn count(trace: &catalyst_contracts::SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

#[test]
fn g19_funding_carry_opens_both_legs() {
    let venue = "hyperliquid";
    // rich on the first bar (>= 0.00001), normal on the second, so the level
    // signal fires exactly once and opens both legs.
    let funding = json!([{"venue": venue, "symbol": "ETH",
        "points": [{"ts": ts(0), "rate": "0.0003"}, {"ts": ts(1), "rate": "0.000005"}]}]);
    let trace = run(&SimulationInput {
        graph: graph("g19_funding_carry.json"),
        config: config(venue, "2000", 2),
        policy: policy("level"),
        market_data: bundle(venue, &["2000", "2000"], funding, json!([])),
    })
    .unwrap();
    assert_eq!(count(&trace, "action_executed"), 2, "long spot + short perp");
    assert_eq!(count(&trace, "action_rejected"), 0);
}

#[test]
fn g24_stop_loss_exits_to_flat() {
    let trace = run(&SimulationInput {
        graph: graph("g24_stop_loss.json"),
        config: config("base", "1000", 3),
        policy: policy("crossing"),
        // Under next_open the initial open fills on bar 1; the stop crosses below
        // 1700 on bar 1 and its sell fills on bar 2 — so a 3-bar run (the last bar
        // still below the stop, no new crossing) is needed for both legs to land.
        market_data: bundle("base", &["2000", "1600", "1600"], json!([]), json!([])),
    })
    .unwrap();
    assert_eq!(count(&trace, "action_executed"), 2, "open + stop-out");
    let eth = trace
        .final_portfolio
        .balances
        .get("base")
        .and_then(|b| b.get("ETH"))
        .map(|s| s.parse::<f64>().unwrap_or(0.0))
        .unwrap_or(0.0);
    assert!(eth.abs() < 1e-9, "flat ETH after stop, got {eth}");
}

#[test]
fn g25_yield_rotation_deposits_when_apr_high() {
    let yields = json!([{"protocol": "aave", "asset": "USDC", "chain": "base", "pool": "usdc",
        "points": [{"ts": ts(0), "apr": "0.06"}, {"ts": ts(1), "apr": "0.06"}]}]);
    let trace = run(&SimulationInput {
        graph: graph("g25_yield_rotation.json"),
        config: config("base", "1000", 2),
        policy: policy("level"),
        market_data: bundle("base", &["2000", "2000"], json!([]), yields),
    })
    .unwrap();
    assert!(count(&trace, "action_executed") >= 1);
    assert_eq!(count(&trace, "action_rejected"), 0);
}

#[test]
fn g26_short_momentum_fires_on_negative_roc() {
    let venue = "hyperliquid";
    let closes = [
        "2000", "1980", "1960", "1940", "1920", "1900", "1880", "1860", "1840", "1820", "1800",
        "1780", "1760",
    ];
    let trace = run(&SimulationInput {
        graph: graph("g26_short_momentum.json"),
        config: config(venue, "5000", closes.len() as i64),
        policy: policy("once_per_backtest"),
        market_data: bundle(venue, &closes, json!([]), json!([])),
    })
    .unwrap();
    assert_eq!(count(&trace, "signal_fired"), 1, "downside momentum fires once");
    assert_eq!(count(&trace, "action_executed"), 1, "short opens");
    assert_eq!(count(&trace, "action_rejected"), 0);
}
