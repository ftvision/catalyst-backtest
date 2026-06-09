//! #41: `next_open` fills use the actual next bar's open (no intra-bar look-ahead).
//! #116: a market order with no next bar (decided on the final bar) does NOT fill —
//! it is deferred and lapses rather than falling back to the final close, which would
//! be same-bar look-ahead.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
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

fn bundle(bars: &[(&str, &str, &str, &str)]) -> MarketDataBundle {
    let points: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| json!({"ts": ts(i as i64), "open": o, "high": h, "low": l, "close": c}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(bars.len() as i64),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

fn config(n_ticks: i64) -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), "1000".to_string());
    let mut init = BTreeMap::new();
    init.insert("base".to_string(), bal);
    BacktestConfig {
        start: START.to_string(),
        end: ts(n_ticks),
        interval: "1h".to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

fn strict() -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None, fills: None, gas: None, signals: None, ordering: None,
        data: None, perps: None, yield_: None,
    }
}

fn buy_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "500", "chain": "base"}}],
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
fn strict_default_fills_at_next_bar_open_not_this_close() {
    // Initial buy decided on bar 0 (close 2000) fills at bar 1's OPEN (2100),
    // not bar 0's close — strict_v1 defaults to next_open. +10bps slippage = 2102.1.
    let trace = run(&SimulationInput {
        graph: buy_graph(),
        config: config(2),
        policy: strict(),
        market_data: bundle(&[("2000", "2005", "1995", "2000"), ("2100", "2110", "2090", "2105")]),
    })
    .unwrap();
    assert_eq!(fill_price(&trace), 2102.1); // 2100 * 1.001, NOT 2000-based
}

#[test]
fn next_open_on_final_bar_does_not_fill() {
    // A single-bar run has no "next" bar to fill against. Under #116 the market order
    // is deferred and then lapses unfilled — it does NOT fall back to the final close
    // (that would be the same-bar look-ahead the deferral exists to prevent). So there
    // is no action_executed, the order is recorded as expired, and the ledger is
    // untouched (full initial cash, no ETH).
    let trace = run(&SimulationInput {
        graph: buy_graph(),
        config: config(1),
        policy: strict(),
        market_data: bundle(&[("1900", "2005", "1895", "2000")]),
    })
    .unwrap();

    assert_eq!(
        trace.events.iter().filter(|e| e.event_type == "action_executed").count(),
        0,
        "a market order on the final bar must not fill (no next open without look-ahead)"
    );
    assert!(
        trace.events.iter().any(|e| e.event_type == "order_expired"),
        "the deferred final-bar order should be recorded as expired; events: {:?}",
        trace.events
    );
    let base = &trace.snapshots[0].portfolio.as_ref().unwrap().balances["base"];
    assert_eq!(base["USDC"].to_string(), "1000", "cash untouched — no fill occurred");
    assert_eq!(base.get("ETH"), None, "no ETH acquired — the order never filled");
}
