//! ADR 0002 step 2: relative action sizing (`Amount::Relative`).

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

/// Config with an arbitrary initial portfolio (venue -> asset -> amount).
fn config(initial: Value, n_ticks: i64) -> BacktestConfig {
    serde_json::from_value(json!({
        "start": START,
        "end": ts(n_ticks),
        "interval": "1h",
        "initial_portfolio": initial,
    }))
    .unwrap()
}

fn strict() -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1"
    }))
    .unwrap()
}

fn graph(v: Value) -> Graph {
    serde_json::from_value(v).unwrap()
}

fn count(trace: &catalyst_contracts::SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

fn approx(s: &str, expected: f64) -> bool {
    s.parse::<f64>().map(|v| (v - expected).abs() < 1e-9).unwrap_or(false)
}

#[test]
fn swap_pct_balance_sells_half_the_from_asset() {
    // sell 50% of a 1.0 ETH holding -> 0.5 ETH left.
    let g = graph(json!({
        "nodes": [{
            "id": "sell-half", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "ETH", "to_asset": "USDC",
                       "amount": {"basis": "pct_balance", "value": "50"}, "chain": "base"}
        }],
        "edges": []
    }));
    let input = SimulationInput {
        graph: g,
        config: config(json!({"base": {"ETH": "1"}}), 2),
        policy: strict(),
        market_data: bundle("base", &["2000", "2000"]),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "action_executed"), 1);
    let eth = &trace.final_portfolio.balances["base"]["ETH"];
    assert!(approx(eth, 0.5), "expected ~0.5 ETH left, got {eth}");
}

#[test]
fn perp_pct_position_reduces_the_open_position() {
    // open a long, then reduce_only 50% of the position notional.
    let g = graph(json!({
        "nodes": [
            {"id": "open", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "long", "size_usd": "1000",
                        "leverage": "1", "chain": "hyperliquid"}},
            {"id": "reduce", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "short",
                        "size_usd": {"basis": "pct_position", "value": "50"},
                        "chain": "hyperliquid", "reduce_only": true}}
        ],
        "edges": [{"from": "open", "to": "reduce"}]
    }));
    let input = SimulationInput {
        graph: g,
        config: config(json!({"hyperliquid": {"USDC": "5000"}}), 2),
        policy: strict(),
        market_data: bundle("hyperliquid", &["2000", "2000"]),
    };
    let trace = run(&input).unwrap();
    // both the open and the relative-sized reduce execute
    assert_eq!(count(&trace, "action_executed"), 2);
    assert_eq!(count(&trace, "action_rejected"), 0);
}

#[test]
fn yield_pct_balance_deposits_a_fraction() {
    let g = graph(json!({
        "nodes": [{
            "id": "dep", "kind": "action", "subtype": "yield_deposit",
            "config": {"chain": "base", "protocol": "aave", "pool": "usdc", "asset": "USDC",
                       "amount": {"basis": "pct_balance", "value": "50"}}
        }],
        "edges": []
    }));
    let mut md = bundle("base", &["2000", "2000"]);
    // yields series so deposit has an APR to accrue against
    md.yields = serde_json::from_value(json!([{"protocol": "aave", "asset": "USDC",
        "chain": "base", "pool": "usdc", "points": [{"ts": ts(0), "apr": "0.05"}]}]))
    .unwrap();
    let input = SimulationInput {
        graph: g,
        config: config(json!({"base": {"USDC": "1000"}}), 2),
        policy: strict(),
        market_data: md,
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "action_executed"), 1);
    assert_eq!(count(&trace, "action_rejected"), 0);
}

#[test]
fn pct_portfolio_is_rejected_until_supported() {
    let g = graph(json!({
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH",
                       "amount": {"basis": "pct_portfolio", "value": "10"}, "chain": "base"}
        }],
        "edges": []
    }));
    let input = SimulationInput {
        graph: g,
        config: config(json!({"base": {"USDC": "1000"}}), 2),
        policy: strict(),
        market_data: bundle("base", &["2000", "2000"]),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "action_rejected"), 1);
    assert!(trace
        .events
        .iter()
        .any(|e| e.reason.as_deref().is_some_and(|r| r.contains("pct_portfolio"))));
}
