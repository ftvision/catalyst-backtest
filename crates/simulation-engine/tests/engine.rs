//! Golden-style engine tests over synthetic market data (no network).

use std::collections::BTreeMap;

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

/// A bundle with an ETH candle path on `venue` (flat OHLC per close), plus gas.
fn eth_bundle(venue: &str, closes: &[&str]) -> MarketDataBundle {
    let points: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    let gas_points: Vec<_> =
        closes.iter().enumerate().map(|(i, _)| json!({"ts": ts(i as i64), "gas_usd": "0.02"})).collect();
    let bundle = json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h",
        "start": ts(0),
        "end": ts(closes.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [],
        "gas": [{"chain": venue, "points": gas_points}],
        "yields": [],
        "providers": [],
        "warnings": []
    });
    serde_json::from_value(bundle).unwrap()
}

fn config(venue: &str, usdc: &str, n_ticks: i64) -> BacktestConfig {
    let mut venue_balances = BTreeMap::new();
    venue_balances.insert("USDC".to_string(), usdc.to_string());
    let mut initial = BTreeMap::new();
    initial.insert(venue.to_string(), venue_balances);
    BacktestConfig {
        start: START.to_string(),
        end: ts(n_ticks),
        interval: "1h".to_string(),
        initial_portfolio: initial,
        execution: None,
    }
}

fn strict_policy() -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None,
        fills: None,
        gas: None,
        signals: None,
        ordering: None,
        data: None,
        perps: None,
        yield_: None,
    }
}

fn graph(value: serde_json::Value) -> Graph {
    serde_json::from_value(value).unwrap()
}

fn count_events(trace: &catalyst_contracts::SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

// --- Initial action + policy metadata ---

#[test]
fn initial_swap_executes_and_trace_carries_policy() {
    let g = graph(json!({
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base"}
        }],
        "edges": []
    }));
    let input = SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: strict_policy(),
        market_data: eth_bundle("base", &["2000", "2000"]),
    };
    let trace = run(&input).unwrap();

    assert_eq!(count_events(&trace, "action_executed"), 1);
    assert_eq!(trace.snapshots.len(), 2);
    // policy metadata is embedded
    assert_eq!(trace.policy.profile, "strict_v1");
    assert_eq!(trace.policy.schema_version, "catalyst.backtest.policy.v1");
    // ended holding ETH on base
    assert!(trace.final_portfolio.balances["base"].contains_key("ETH"));
    // the whole trace round-trips as the contract type
    let s = serde_json::to_string(&trace).unwrap();
    let _back: catalyst_contracts::SimulationTrace = serde_json::from_str(&s).unwrap();
}

// --- Threshold crossing fires once; repeated only on re-cross ---

fn signal_buy_graph() -> serde_json::Value {
    json!({
        "nodes": [
            {"id": "eth-below-1800", "kind": "signal", "subtype": "price_threshold",
             "config": {"symbol": "ETH", "operator": "<", "threshold": "1800"}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base"}}
        ],
        "edges": [{"from": "eth-below-1800", "to": "buy"}]
    })
}

#[test]
fn threshold_crossing_fires_once_while_condition_holds() {
    // dips below 1800 once and stays there
    let input = SimulationInput {
        graph: graph(signal_buy_graph()),
        config: config("base", "1000", 3),
        policy: strict_policy(),
        market_data: eth_bundle("base", &["2000", "1700", "1700"]),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count_events(&trace, "signal_fired"), 1);
    assert_eq!(count_events(&trace, "action_executed"), 1);
}

#[test]
fn signal_refires_on_recross() {
    // below, back above, below again -> two crossings
    let input = SimulationInput {
        graph: graph(signal_buy_graph()),
        config: config("base", "1000", 4),
        policy: strict_policy(),
        market_data: eth_bundle("base", &["2000", "1700", "2000", "1700"]),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count_events(&trace, "signal_fired"), 2);
    assert_eq!(count_events(&trace, "action_executed"), 2);
}

// --- Action chaining ---

#[test]
fn action_chains_to_next_action() {
    let g = graph(json!({
        "nodes": [
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "hyperliquid"}},
            {"id": "sell", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "ETH", "to_asset": "USDC", "amount": "0.04", "chain": "hyperliquid"}}
        ],
        "edges": [{"from": "buy", "to": "sell"}]
    }));
    let input = SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 1),
        policy: strict_policy(),
        market_data: eth_bundle("hyperliquid", &["2000"]),
    };
    let trace = run(&input).unwrap();
    // both the initial buy and the chained sell execute on the first tick
    assert_eq!(count_events(&trace, "action_executed"), 2);
    let kinds: Vec<_> = trace
        .events
        .iter()
        .filter(|e| e.event_type == "action_executed")
        .filter_map(|e| e.node_id.clone())
        .collect();
    assert_eq!(kinds, vec!["buy", "sell"]);
}

// --- Rejected actions ---

#[test]
fn selling_more_than_held_is_rejected() {
    let g = graph(json!({
        "nodes": [{
            "id": "sell", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "ETH", "to_asset": "USDC", "amount": "0.04", "chain": "hyperliquid"}
        }],
        "edges": []
    }));
    let input = SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 1), // USDC only, no ETH
        policy: strict_policy(),
        market_data: eth_bundle("hyperliquid", &["2000"]),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count_events(&trace, "action_rejected"), 1);
    assert_eq!(count_events(&trace, "action_executed"), 0);
    // portfolio unchanged: still 1000 USDC, no ETH
    assert_eq!(trace.final_portfolio.balances["hyperliquid"]["USDC"], "1000");
}

// --- Perp round trip with funding does not panic and settles ---

#[test]
fn perp_open_and_close_via_signals_runs() {
    let g = graph(json!({
        "nodes": [
            {"id": "open", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "long", "size_usd": "500", "leverage": "5",
                        "chain": "hyperliquid", "order_type": "market", "reduce_only": false}},
            {"id": "eth-above-2300", "kind": "signal", "subtype": "price_threshold",
             "config": {"symbol": "ETH", "operator": ">", "threshold": "2300"}},
            {"id": "close", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "short", "size_usd": "500",
                        "chain": "hyperliquid", "order_type": "market", "reduce_only": true}}
        ],
        "edges": [{"from": "eth-above-2300", "to": "close"}]
    }));
    let input = SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 3),
        policy: strict_policy(),
        market_data: eth_bundle("hyperliquid", &["2000", "2400", "2400"]),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count_events(&trace, "action_executed"), 2); // open + close
    assert!(trace.final_portfolio.perp_positions.is_empty()); // closed out
}
