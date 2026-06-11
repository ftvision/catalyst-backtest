//! #49: graph variables are resolved by the compiler, so the engine executes
//! with concrete values.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::json;

const START: &str = "2024-01-01T00:00:00Z";

fn ts(i: i64) -> String {
    chrono::DateTime::from_timestamp(1_704_067_200 + i * 3600, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

#[test]
fn variable_amount_is_resolved_and_executed() {
    let g: Graph = serde_json::from_value(json!({
        "variables": {"size": "100"},
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "$size", "chain": "base"}
        }],
        "edges": []
    }))
    .unwrap();

    let md: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(2),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": [
            {"ts": ts(0), "open": "2000", "high": "2000", "low": "2000", "close": "2000"},
            {"ts": ts(1), "open": "2000", "high": "2000", "low": "2000", "close": "2000"}
        ]}],
        "gas": [{"chain": "base", "points": [{"ts": ts(0), "gas_usd": "0.0"}]}]
    }))
    .unwrap();

    let mut bals = BTreeMap::new();
    bals.insert("USDC".to_string(), "1000".to_string());
    let mut initial = BTreeMap::new();
    initial.insert("base".to_string(), bals);
    let config = BacktestConfig {
        start: START.to_string(),
        // Last bar is ts(1); #167 enforces the window matches the data.
        end: ts(1),
        interval: "1h".to_string(),
        initial_portfolio: initial,
        execution: None,
    };
    let policy: SimulationPolicy = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1", "profile": "strict_v1"
    }))
    .unwrap();

    let trace = run(&SimulationInput { graph: g, config, policy, market_data: md }).unwrap();
    assert_eq!(trace.events.iter().filter(|e| e.event_type == "action_executed").count(), 1);
    // spent the resolved $size = 100 USDC (plus a few bps of fee)
    let usdc = trace.final_portfolio.balances["base"]["USDC"].parse::<f64>().unwrap();
    assert!((899.0..900.0).contains(&usdc), "expected ~900 USDC left, got {usdc}");
}
