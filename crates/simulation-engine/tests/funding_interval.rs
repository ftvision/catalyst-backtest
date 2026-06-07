//! A 4h-tick perp backtest must accrue *all* hourly funding within each bar, not
//! just the funding point that lands on the tick (the pre-fix exact-match bug).

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const EPOCH: i64 = 1_704_067_200;

fn iso(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn bundle() -> MarketDataBundle {
    // 4h candles (flat 2000) at 0h, 4h, 8h -> engine ticks every 4h.
    let candles: Vec<Value> = [0, 4, 8]
        .iter()
        .map(|h| json!({"ts": iso(EPOCH + h * 3600), "open": "2000", "high": "2000", "low": "2000", "close": "2000"}))
        .collect();
    // hourly funding, 0.001 each, at 1h..8h.
    let funding: Vec<Value> = (1..=8)
        .map(|h| json!({"ts": iso(EPOCH + h * 3600), "rate": "0.001"}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "4h", "start": iso(EPOCH), "end": iso(EPOCH + 8 * 3600),
        "candles": [{"venue": "hyperliquid", "symbol": "ETH", "quote": "USD", "points": candles}],
        "funding": [{"venue": "hyperliquid", "symbol": "ETH", "points": funding}],
        "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

fn config() -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), "2000".to_string());
    let mut init = BTreeMap::new();
    init.insert("hyperliquid".to_string(), bal);
    BacktestConfig {
        start: START.to_string(),
        end: iso(EPOCH + 8 * 3600),
        interval: "4h".to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

fn policy() -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None, fills: None, gas: None, signals: None, ordering: None,
        data: None, perps: None, yield_: None,
    }
}

fn open_long_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
            "config": {"symbol": "ETH", "side": "long", "size_usd": "1000", "leverage": "1",
                       "chain": "hyperliquid", "order_type": "market", "reduce_only": false}}],
        "edges": []
    }))
    .unwrap()
}

fn funding_events(t: &SimulationTrace) -> Vec<&catalyst_contracts::trace::Event> {
    t.events.iter().filter(|e| e.event_type == "funding_applied").collect()
}

#[test]
fn four_hour_tick_sums_all_hourly_funding_in_the_bar() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle(),
    })
    .unwrap();

    // Position opens at tick 0 (no funding yet); at tick 1 (4h later) funding for
    // hours 1,2,3,4 (4 x 0.001 = 0.004) accrues — not just the 0.001 at hour 4.
    let fe = funding_events(&trace);
    assert!(!fe.is_empty(), "expected funding to accrue");
    let first = fe[0];
    let rate = first.detail.as_ref().unwrap()["rate"].as_str().unwrap();
    assert_eq!(rate, "0.004", "4h bar should sum 4 hourly funding points");
    // notional ~= 1000 (size bought at 2002 w/ 10bps slippage, marked at 2000);
    // payment = 0.004 * ~999 ~= 3.996 — i.e. ~4x the single-point bug value (~1.0).
    let pay: f64 = first.detail.as_ref().unwrap()["payment_usd"].as_str().unwrap().parse().unwrap();
    assert!((3.9..4.0).contains(&pay), "payment was {pay} (expected ~3.996, 4 hourly points)");
}
