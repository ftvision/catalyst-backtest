//! Negative reference constants: a rate-of-change signal that fires on downside
//! momentum (`roc(12) <= -0.05`) and opens a short.

use std::collections::BTreeMap;
use std::path::PathBuf;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::json;

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

#[test]
fn downside_momentum_with_negative_roc_threshold_opens_a_short() {
    let venue = "hyperliquid";
    // 13 falling bars: roc(12) becomes <= -0.05 once warmed up.
    let closes = [
        "2000", "1980", "1960", "1940", "1920", "1900", "1880", "1860", "1840", "1820", "1800",
        "1780", "1760",
    ];
    let points: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    let gas: Vec<_> =
        closes.iter().enumerate().map(|(i, _)| json!({"ts": ts(i as i64), "gas_usd": "0.0"})).collect();
    let md: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(closes.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [{"chain": venue, "points": gas}], "yields": [],
        "providers": [], "warnings": []
    }))
    .unwrap();

    let mut bals = BTreeMap::new();
    bals.insert("USDC".to_string(), "5000".to_string());
    let mut initial = BTreeMap::new();
    initial.insert(venue.to_string(), bals);
    let config = BacktestConfig {
        start: START.to_string(),
        end: ts(closes.len() as i64),
        interval: "1h".to_string(),
        initial_portfolio: initial,
        execution: None,
    };
    let policy: SimulationPolicy = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "signals": {"trigger": "once_per_backtest"}
    }))
    .unwrap();

    let trace = run(&SimulationInput {
        graph: example("graph.short-momentum.json"),
        config,
        policy,
        market_data: md,
    })
    .unwrap();

    let count = |k: &str| trace.events.iter().filter(|e| e.event_type == k).count();
    assert_eq!(count("signal_fired"), 1, "downside momentum should fire once");
    assert_eq!(count("action_executed"), 1, "the short should open");
    assert_eq!(count("action_rejected"), 0);
}
