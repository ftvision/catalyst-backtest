//! #40: end-to-end AMM price-impact — a swap fills at the constant-product price
//! derived from the bundle's pool-reserve (liquidity) series, not fixed-bps.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::json;

const START: &str = "2024-01-01T00:00:00Z";

fn bundle(with_liquidity: bool) -> MarketDataBundle {
    let mut b = json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": START, "end": "2024-01-01T02:00:00Z",
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": [
            {"ts": "2024-01-01T00:00:00Z", "open": "2000", "high": "2000", "low": "2000", "close": "2000"},
            {"ts": "2024-01-01T01:00:00Z", "open": "2000", "high": "2000", "low": "2000", "close": "2000"}
        ]}],
        "gas": [], "funding": [], "yields": [], "providers": [], "warnings": []
    });
    if with_liquidity {
        // small pool: 100 ETH / 200_000 USDC -> buying 2000 USDC pushes avg price to 2020
        b["liquidity"] = json!([{"venue": "base", "symbol": "ETH", "points": [
            {"ts": "2024-01-01T00:00:00Z", "reserve_base": "100", "reserve_quote": "200000"}
        ]}]);
    }
    serde_json::from_value(b).unwrap()
}

fn config() -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), "5000".to_string());
    let mut init = BTreeMap::new();
    init.insert("base".to_string(), bal);
    BacktestConfig {
        start: START.to_string(),
        end: "2024-01-01T02:00:00Z".to_string(),
        interval: "1h".to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

/// strict_v1 with the slippage model overridden to amm_price_impact.
fn amm_policy() -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "fills": { "slippage": { "model": "amm_price_impact" } }
    }))
    .unwrap()
}

fn buy_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "2000", "chain": "base"}}],
        "edges": []
    }))
    .unwrap()
}

fn fill_price(trace: &SimulationTrace) -> f64 {
    trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed")
        .and_then(|e| e.detail.as_ref())
        .and_then(|d| d.get("price"))
        .and_then(|v| v.as_str())
        .expect("a fill price")
        .parse()
        .unwrap()
}

#[test]
fn amm_price_impact_uses_pool_reserves() {
    let trace = run(&SimulationInput {
        graph: buy_graph(),
        config: config(),
        policy: amm_policy(),
        market_data: bundle(true),
    })
    .unwrap();
    // (rq + amount)/rb = (200000 + 2000)/100 = 2020
    assert_eq!(fill_price(&trace), 2020.0);
}

#[test]
fn amm_without_liquidity_falls_back_to_fixed_bps() {
    // same policy, but no pool series -> falls back to the configured bps
    // (strict_v1's 10 bps), a real cost rather than zero slippage (#136).
    let trace = run(&SimulationInput {
        graph: buy_graph(),
        config: config(),
        policy: amm_policy(),
        market_data: bundle(false),
    })
    .unwrap();
    assert_eq!(fill_price(&trace), 2002.0);
}
