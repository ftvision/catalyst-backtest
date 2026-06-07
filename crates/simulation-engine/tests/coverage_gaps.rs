//! #42: a required candle series with an interior hole fails under
//! `missing_required = fail` (strict) and warns under forward-fill (research).

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const START_EPOCH: i64 = 1_704_067_200;

fn ts(i: i64) -> String {
    chrono::DateTime::from_timestamp(START_EPOCH + i * 3600, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Candles at the given hour indices (a hole = a skipped index).
fn holed_bundle(hours: &[i64]) -> MarketDataBundle {
    let points: Vec<Value> = hours
        .iter()
        .map(|&i| json!({"ts": ts(i), "open": "2000", "high": "2000", "low": "2000", "close": "2000"}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(4),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

fn config() -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), "1000".to_string());
    let mut init = BTreeMap::new();
    init.insert("base".to_string(), bal);
    BacktestConfig {
        start: START.to_string(),
        end: ts(4),
        interval: "1h".to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

fn policy(profile: &str) -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: profile.to_string(),
        balance: None, fills: None, gas: None, signals: None, ordering: None,
        data: None, perps: None, yield_: None,
    }
}

fn swap_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base"}}],
        "edges": []
    }))
    .unwrap()
}

#[test]
fn strict_fails_on_interior_gap() {
    // hours 0,1,3 -> hour 2 missing inside the required base/ETH series
    let err = run(&SimulationInput {
        graph: swap_graph(),
        config: config(),
        policy: policy("strict_v1"),
        market_data: holed_bundle(&[0, 1, 3, 4]),
    })
    .unwrap_err();
    assert!(format!("{err}").contains("missing"), "got: {err}");
}

#[test]
fn research_warns_on_interior_gap() {
    let trace = run(&SimulationInput {
        graph: swap_graph(),
        config: config(),
        policy: policy("research_v1"), // missing_required = forward_fill
        market_data: holed_bundle(&[0, 1, 3, 4]),
    })
    .unwrap();
    assert!(trace.warnings.iter().any(|w| w.contains("missing bar")), "warnings: {:?}", trace.warnings);
}

#[test]
fn contiguous_series_runs_clean() {
    let trace = run(&SimulationInput {
        graph: swap_graph(),
        config: config(),
        policy: policy("strict_v1"),
        market_data: holed_bundle(&[0, 1, 2, 3, 4]),
    })
    .unwrap();
    assert!(!trace.warnings.iter().any(|w| w.contains("missing bar")));
}
