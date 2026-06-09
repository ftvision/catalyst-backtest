//! Resting limit-order semantics: rest, fill-when-touched, gap-aware fill price,
//! next-bar eligibility, time-in-force expiry, and reduce-only take-profit.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const START_EPOCH: i64 = 1_704_067_200;
const STEP: i64 = 3600;

fn ts(i: i64) -> String {
    let epoch = START_EPOCH + i * STEP;
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A bundle with an ETH candle path of explicit (open, high, low, close) bars.
fn ohlc_bundle(venue: &str, bars: &[(&str, &str, &str, &str)]) -> MarketDataBundle {
    let points: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| json!({"ts": ts(i as i64), "open": o, "high": h, "low": l, "close": c}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h",
        "start": ts(0),
        "end": ts(bars.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [],
        "gas": [{"chain": venue, "points": []}],
        "yields": [],
        "providers": [],
        "warnings": []
    }))
    .unwrap()
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

fn graph(value: Value) -> Graph {
    serde_json::from_value(value).unwrap()
}

fn count(trace: &SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

fn first(trace: &SimulationTrace, kind: &str) -> catalyst_contracts::trace::Event {
    trace.events.iter().find(|e| e.event_type == kind).cloned().expect("event present")
}

fn detail<'a>(e: &'a catalyst_contracts::trace::Event, key: &str) -> &'a Value {
    e.detail.as_ref().unwrap().get(key).unwrap_or(&Value::Null)
}

fn perp_limit_graph(side: &str, limit: &str, reduce_only: bool) -> Value {
    json!({"id": if reduce_only {"tp"} else {"open"}, "kind": "action", "subtype": "perp_order",
        "config": {"symbol": "ETH", "side": side, "size_usd": "500", "leverage": "2",
                   "chain": "hyperliquid", "order_type": "limit", "limit_price": limit,
                   "reduce_only": reduce_only}})
}

// --- core: rest then fill when a later bar touches ---

#[test]
fn perp_limit_rests_then_fills_when_touched() {
    // Place a limit long at 1900 on bar 0 (trading ~2000). Bar 1 dips to a low of
    // 1850, touching the limit -> fills at exactly 1900 (no taker slippage).
    let g = graph(json!({ "nodes": [perp_limit_graph("long", "1900", false)], "edges": [] }));
    let input = SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 2),
        policy: strict_policy(),
        market_data: ohlc_bundle(
            "hyperliquid",
            &[("2000", "2010", "1990", "2000"), ("1980", "1985", "1850", "1900")],
        ),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "order_placed"), 1);
    assert_eq!(count(&trace, "order_filled"), 1);
    assert_eq!(count(&trace, "action_executed"), 0); // the perp itself is a limit fill
    let filled = first(&trace, "order_filled");
    assert_eq!(detail(&filled, "price"), "1900");
    assert_eq!(filled.ts, ts(1)); // filled on bar 1, not the placement bar
    let pos = &trace.final_portfolio.perp_positions;
    assert_eq!(pos.len(), 1);
    assert_eq!(pos[0].entry_price, "1900");
}

// --- next-bar eligibility: the placement bar is never used ---

#[test]
fn limit_does_not_fill_on_placement_bar() {
    // Bar 0 (the placement bar) already dips to 1850, which would satisfy a 1900
    // buy — but a resting order is only eligible from bar 1. Bar 1 stays at 2000,
    // so it never fills and expires (GTC) at the end of the run.
    let g = graph(json!({ "nodes": [perp_limit_graph("long", "1900", false)], "edges": [] }));
    let input = SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 2),
        policy: strict_policy(),
        market_data: ohlc_bundle(
            "hyperliquid",
            &[("2000", "2010", "1850", "2000"), ("2000", "2010", "1990", "2000")],
        ),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "order_filled"), 0);
    assert_eq!(count(&trace, "order_expired"), 1); // GTC leftover at run end
    assert!(trace.final_portfolio.perp_positions.is_empty());
}

// --- gap-through fills at the (more favorable) open ---

#[test]
fn limit_gap_through_fills_at_open() {
    // Bar 1 gaps open below the 1900 buy limit (opens 1850) -> fill at the open
    // 1850, not the limit, because the trader got a better price.
    let g = graph(json!({ "nodes": [perp_limit_graph("long", "1900", false)], "edges": [] }));
    let input = SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 2),
        policy: strict_policy(),
        market_data: ohlc_bundle(
            "hyperliquid",
            &[("2000", "2010", "1990", "2000"), ("1850", "1860", "1820", "1840")],
        ),
    };
    let trace = run(&input).unwrap();
    let filled = first(&trace, "order_filled");
    assert_eq!(detail(&filled, "price"), "1850");
}

// --- time-in-force: good-til-N-bars expires ---

#[test]
fn limit_expires_after_n_bars() {
    // good_til_bars = 1: eligible only on bar 1. It never touches (1900 buy, lows
    // stay 1990), so on bar 2 (placed_index 0 + 1 elapsed) it expires.
    let mut node = perp_limit_graph("long", "1900", false);
    node["config"]["time_in_force"] = json!("good_til_bars");
    node["config"]["expire_after_bars"] = json!(1);
    let g = graph(json!({ "nodes": [node], "edges": [] }));
    let input = SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 3),
        policy: strict_policy(),
        market_data: ohlc_bundle(
            "hyperliquid",
            &[
                ("2000", "2010", "1990", "2000"),
                ("2000", "2010", "1990", "2000"),
                ("2000", "2010", "1990", "2000"),
            ],
        ),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "order_filled"), 0);
    assert_eq!(count(&trace, "order_expired"), 1);
    let expired = first(&trace, "order_expired");
    assert_eq!(expired.reason.as_deref(), Some("time_in_force elapsed"));
    assert_eq!(expired.ts, ts(2));
}

// --- reduce-only limit = take-profit ---

#[test]
fn reduce_only_limit_take_profit_fills() {
    // Open a market long, then chain a take-profit (reduce-only) sell limit at 2200.
    // Under next_open (#116) the market entry decided on bar 0 fills on bar 1's open,
    // and only THEN — once the position exists — can the reduce-only take-profit be
    // placed (a reduce-only limit with no open position is rejected). The TP rests
    // from bar 1 and is eligible from bar 2, where the rally to a high of 2300
    // touches it and the position closes at 2200.
    let g = graph(json!({
        "nodes": [
            {"id": "open", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "long", "size_usd": "500", "leverage": "2",
                        "chain": "hyperliquid", "order_type": "market", "reduce_only": false}},
            perp_limit_graph("short", "2200", true)
        ],
        "edges": [{"from": "open", "to": "tp"}]
    }));
    let input = SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 3),
        policy: strict_policy(),
        market_data: ohlc_bundle(
            "hyperliquid",
            &[
                ("2000", "2010", "1990", "2000"),
                ("2100", "2110", "2090", "2100"),
                ("2200", "2300", "2190", "2250"),
            ],
        ),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "action_executed"), 1); // the market open (fills on bar 1)
    assert_eq!(count(&trace, "order_placed"), 1); // the take-profit (placed on bar 1)
    assert_eq!(count(&trace, "order_filled"), 1); // touched at 2200 on bar 2
    let filled = first(&trace, "order_filled");
    assert_eq!(detail(&filled, "price"), "2200");
    assert!(trace.final_portfolio.perp_positions.is_empty()); // closed out
}

// --- swap limit orders work too ---

#[test]
fn swap_limit_fills_when_touched() {
    let g = graph(json!({
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base",
                       "order_type": "limit", "limit_price": "1900"}
        }],
        "edges": []
    }));
    let input = SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: strict_policy(),
        market_data: ohlc_bundle(
            "base",
            &[("2000", "2010", "1990", "2000"), ("1980", "1985", "1850", "1900")],
        ),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "order_filled"), 1);
    let filled = first(&trace, "order_filled");
    assert_eq!(detail(&filled, "price"), "1900");
    assert!(trace.final_portfolio.balances["base"].contains_key("ETH"));
}

// --- a limit order missing its price is rejected at placement ---

#[test]
fn limit_without_price_is_rejected() {
    let g = graph(json!({
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base",
                       "order_type": "limit"}
        }],
        "edges": []
    }));
    let input = SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: strict_policy(),
        market_data: ohlc_bundle("base", &[("2000", "2010", "1990", "2000"), ("2000", "2010", "1990", "2000")]),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "order_placed"), 0);
    assert_eq!(count(&trace, "action_rejected"), 1);
}
