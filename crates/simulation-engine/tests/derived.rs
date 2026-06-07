//! ADR 0002 step 3: derived signal sources (moving averages, breakouts) and
//! their warmup behavior.

use std::collections::BTreeMap;

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

fn bundle(venue: &str, closes: &[&str]) -> MarketDataBundle {
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
        "funding": [], "gas": [{"chain": venue, "points": gas}], "yields": [],
        "providers": [], "warnings": []
    }))
    .unwrap()
}

fn config(n_ticks: i64) -> BacktestConfig {
    let mut bals = BTreeMap::new();
    bals.insert("USDC".to_string(), "10000".to_string());
    let mut initial = BTreeMap::new();
    initial.insert("base".to_string(), bals);
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

fn graph(v: Value) -> Graph {
    serde_json::from_value(v).unwrap()
}

fn count(trace: &catalyst_contracts::SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

fn buy() -> Value {
    json!({"id": "buy", "kind": "action", "subtype": "swap",
           "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}})
}

fn price_vs_sma_graph(window: u32, op: &str) -> Value {
    json!({
        "nodes": [
            {"id": "below-ma", "kind": "signal", "subtype": "threshold",
             "config": {
                 "source": {"kind": "price", "symbol": "ETH", "venue": "base"},
                 "operator": op,
                 "reference": {"source": {"kind": "derived",
                     "of": {"kind": "price", "symbol": "ETH", "venue": "base"},
                     "transform": "sma", "window": window}}
             }},
            buy()
        ],
        "edges": [{"from": "below-ma", "to": "buy"}]
    })
}

#[test]
fn price_crossing_below_its_moving_average_fires_once() {
    // sma(3); price dips below the average at tick 3 only.
    let input = SimulationInput {
        graph: graph(price_vs_sma_graph(3, "<")),
        config: config(5),
        policy: policy("crossing"),
        market_data: bundle("base", &["100", "100", "100", "90", "100"]),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "signal_fired"), 1);
    assert_eq!(count(&trace, "action_executed"), 1);
}

#[test]
fn derived_signal_does_not_fire_before_warmup() {
    // window 5 but only 3 bars of history -> never enough samples -> no fire.
    let input = SimulationInput {
        graph: graph(price_vs_sma_graph(5, "<")),
        config: config(3),
        policy: policy("level"),
        market_data: bundle("base", &["100", "100", "100"]),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "signal_fired"), 0);
    assert_eq!(count(&trace, "action_executed"), 0);
}
