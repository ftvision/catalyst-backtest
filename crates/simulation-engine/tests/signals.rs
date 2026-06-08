//! ADR 0002 step 1: generalized threshold surface (funding/yield/price sources,
//! `price_threshold` sugar equivalence, variable references) and the
//! repeat/cooldown firing semantics the engine now honors.

use std::collections::BTreeMap;

use catalyst_contracts::policy::SignalPolicy;
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

/// Bundle with a flat ETH candle path on `venue` (drives ticks), plus gas, and
/// caller-supplied funding/yields series.
fn bundle(venue: &str, closes: &[&str], funding: Value, yields: Value) -> MarketDataBundle {
    let points: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    let gas_points: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, _)| json!({"ts": ts(i as i64), "gas_usd": "0.02"}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h",
        "start": ts(0),
        "end": ts(closes.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": funding,
        "gas": [{"chain": venue, "points": gas_points}],
        "yields": yields,
        "providers": [],
        "warnings": []
    }))
    .unwrap()
}

fn yield_only_bundle(yields: Value) -> MarketDataBundle {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h",
        "start": ts(0),
        "end": ts(3),
        "candles": [],
        "funding": [],
        "gas": [],
        "yields": yields,
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

fn sig(
    trigger: Option<&str>,
    repeat: Option<&str>,
    cooldown: Option<&str>,
    max_count: Option<u32>,
) -> SignalPolicy {
    SignalPolicy {
        trigger: trigger.map(String::from),
        repeat: repeat.map(String::from),
        cooldown: cooldown.map(String::from),
        max_count,
    }
}

fn policy_with_signals(signals: SignalPolicy) -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None,
        fills: None,
        gas: None,
        signals: Some(signals),
        ordering: None,
        data: None,
        perps: None,
        yield_: None,
    }
}

fn graph(v: Value) -> Graph {
    serde_json::from_value(v).unwrap()
}

fn count(trace: &catalyst_contracts::SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

fn price_buy_graph() -> Value {
    json!({
        "nodes": [
            {"id": "below", "kind": "signal", "subtype": "price_threshold",
             "config": {"symbol": "ETH", "operator": "<", "threshold": "1800"}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}}
        ],
        "edges": [{"from": "below", "to": "buy"}]
    })
}

// --- new sources ---

#[test]
fn funding_source_threshold_reads_funding_and_fires() {
    let venue = "hyperliquid";
    let funding = json!([{"venue": venue, "symbol": "ETH", "points": [
        {"ts": ts(0), "rate": "0.0002"},
        {"ts": ts(1), "rate": "0.00005"},
        {"ts": ts(2), "rate": "0.0003"}
    ]}]);
    let g = json!({
        "nodes": [
            {"id": "rich", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "funding", "venue": venue, "symbol": "ETH"},
                        "operator": ">=", "reference": {"const": "0.0001"}}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": venue}}
        ],
        "edges": [{"from": "rich", "to": "buy"}]
    });
    let input = SimulationInput {
        graph: graph(g),
        config: config(venue, "1000", 3),
        policy: policy_with_signals(sig(Some("level"), None, None, None)),
        market_data: bundle(venue, &["2000", "2000", "2000"], funding, json!([])),
    };
    let trace = run(&input).unwrap();
    // funding rich at ticks 0 and 2; level fires whenever the condition holds.
    assert_eq!(count(&trace, "signal_fired"), 2);
}

#[test]
fn yield_source_threshold_reads_apr_and_fires() {
    let venue = "base";
    let yields = json!([{"protocol": "aave", "asset": "USDC", "chain": "base", "pool": "usdc",
        "points": [
            {"ts": ts(0), "apr": "0.06"},
            {"ts": ts(1), "apr": "0.04"},
            {"ts": ts(2), "apr": "0.07"}
        ]}]);
    let g = json!({
        "nodes": [
            {"id": "high-apr", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "yield", "protocol": "aave", "asset": "USDC",
                                   "chain": "base", "pool": "usdc"},
                        "operator": ">=", "reference": {"const": "0.05"}}},
            {"id": "deposit", "kind": "action", "subtype": "yield_deposit",
             "config": {"chain": "base", "protocol": "aave", "pool": "usdc",
                        "asset": "USDC", "amount": "100"}}
        ],
        "edges": [{"from": "high-apr", "to": "deposit"}]
    });
    let input = SimulationInput {
        graph: graph(g),
        config: config(venue, "1000", 3),
        policy: policy_with_signals(sig(Some("level"), None, None, None)),
        market_data: bundle(venue, &["2000", "2000", "2000"], json!([]), yields),
    };
    let trace = run(&input).unwrap();
    // apr >= 5% at ticks 0 and 2.
    assert_eq!(count(&trace, "signal_fired"), 2);
}

#[test]
fn yield_source_without_candles_drives_ticks() {
    let yields = json!([{"protocol": "aave", "asset": "USDC", "chain": "base", "pool": "usdc",
        "points": [
            {"ts": ts(0), "apr": "0.06"},
            {"ts": ts(1), "apr": "0.04"},
            {"ts": ts(2), "apr": "0.07"}
        ]}]);
    let g = json!({
        "nodes": [
            {"id": "high-apr", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "yield", "protocol": "aave", "asset": "USDC",
                                   "chain": "base", "pool": "usdc"},
                        "operator": ">=", "reference": {"const": "0.05"}}},
            {"id": "deposit", "kind": "action", "subtype": "yield_deposit",
             "config": {"chain": "base", "protocol": "aave", "pool": "usdc",
                        "asset": "USDC", "amount": "100"}}
        ],
        "edges": [{"from": "high-apr", "to": "deposit"}]
    });
    let trace = run(&SimulationInput {
        graph: graph(g),
        config: config("base", "1000", 3),
        policy: policy_with_signals(sig(Some("level"), None, None, None)),
        market_data: yield_only_bundle(yields),
    })
    .unwrap();

    assert_eq!(trace.snapshots.len(), 3);
    assert_eq!(count(&trace, "signal_fired"), 2);
    assert_eq!(count(&trace, "action_executed"), 2);
    assert!(trace.warnings.is_empty(), "warnings: {:?}", trace.warnings);
}

// --- price_threshold sugar equivalence + variables ---

#[test]
fn threshold_price_source_matches_price_threshold() {
    let g = json!({
        "nodes": [
            {"id": "below", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "price", "symbol": "ETH"},
                        "operator": "<", "reference": {"const": "1800"}}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}}
        ],
        "edges": [{"from": "below", "to": "buy"}]
    });
    let input = SimulationInput {
        graph: graph(g),
        config: config("base", "1000", 3),
        policy: policy_with_signals(sig(Some("crossing"), None, None, None)),
        market_data: bundle("base", &["2000", "1700", "1700"], json!([]), json!([])),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "signal_fired"), 1);
    assert_eq!(count(&trace, "action_executed"), 1);
}

#[test]
fn reference_var_resolves_from_graph_variables() {
    let g = json!({
        "variables": {"floor": "1800"},
        "nodes": [
            {"id": "below", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "price", "symbol": "ETH"},
                        "operator": "<", "reference": {"var": "floor"}}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}}
        ],
        "edges": [{"from": "below", "to": "buy"}]
    });
    let input = SimulationInput {
        graph: graph(g),
        config: config("base", "1000", 3),
        policy: policy_with_signals(sig(Some("crossing"), None, None, None)),
        market_data: bundle("base", &["2000", "1700", "1700"], json!([]), json!([])),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "signal_fired"), 1);
}

// --- repeat / cooldown semantics ---

#[test]
fn crossing_with_cooldown_suppresses_a_refire() {
    // Three would-be crossings; a 3h cooldown suppresses the middle one.
    let closes = ["1700", "2000", "1700", "2000", "1700"];
    let input = SimulationInput {
        graph: graph(price_buy_graph()),
        config: config("base", "1000", 5),
        policy: policy_with_signals(sig(Some("crossing_with_cooldown"), None, Some("3h"), None)),
        market_data: bundle("base", &closes, json!([]), json!([])),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "signal_fired"), 2);
}

#[test]
fn repeat_never_fires_at_most_once() {
    let closes = ["1700", "2000", "1700"]; // two crossings
    let input = SimulationInput {
        graph: graph(price_buy_graph()),
        config: config("base", "1000", 3),
        policy: policy_with_signals(sig(Some("crossing"), Some("never"), None, None)),
        market_data: bundle("base", &closes, json!([]), json!([])),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "signal_fired"), 1);
}

#[test]
fn repeat_max_count_caps_fires() {
    let closes = ["1700", "2000", "1700", "2000", "1700"]; // three crossings
    let input = SimulationInput {
        graph: graph(price_buy_graph()),
        config: config("base", "1000", 5),
        policy: policy_with_signals(sig(Some("crossing"), Some("max_count"), None, Some(2))),
        market_data: bundle("base", &closes, json!([]), json!([])),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "signal_fired"), 2);
}

// --- composition: all / any / not combinators (ADR 0002 step 4) ---

fn leaf(id: &str, op: &str, threshold: &str) -> Value {
    json!({"id": id, "kind": "signal", "subtype": "threshold",
           "config": {"source": {"kind": "price", "symbol": "ETH"},
                      "operator": op, "reference": {"const": threshold}}})
}

fn buy_node() -> Value {
    json!({"id": "buy", "kind": "action", "subtype": "swap",
           "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}})
}

#[test]
fn all_combinator_fires_only_when_every_input_true() {
    // band: 1000 < ETH < 2000
    let g = json!({
        "nodes": [
            leaf("hi", "<", "2000"),
            leaf("lo", ">", "1000"),
            {"id": "band", "kind": "signal", "subtype": "all", "config": {}},
            buy_node()
        ],
        "edges": [
            {"from": "hi", "to": "band"},
            {"from": "lo", "to": "band"},
            {"from": "band", "to": "buy"}
        ]
    });
    let input = SimulationInput {
        graph: graph(g),
        config: config("base", "1000", 5),
        policy: policy_with_signals(sig(Some("level"), None, None, None)),
        // in-band, hi-out, lo-out, in-band, hi-out (extra bar so the tick-3
        // in-band swap has a next bar to fill against under #116 next_open
        // deferral; the tick-5 bar is out-of-band so it adds no extra signal).
        market_data: bundle("base", &["1500", "2500", "800", "1500", "2500"], json!([]), json!([])),
    };
    let trace = run(&input).unwrap();
    // Signal evaluation is unchanged: fires in-band at ticks 0 and 3 (ticks 1,2,4
    // are out-of-band) -> 2 fires.
    assert_eq!(count(&trace, "signal_fired"), 2);
    // #116: each market swap is booked at the NEXT bar's open. The tick-0 swap
    // fills at tick 1; the tick-3 swap now fills at tick 4 (previously it was the
    // final bar and would be dropped). Both execute -> 2 actions.
    assert_eq!(count(&trace, "action_executed"), 2);
}

#[test]
fn any_combinator_fires_when_some_input_true() {
    let g = json!({
        "nodes": [
            leaf("crash", "<", "1200"),
            leaf("spike", ">", "2800"),
            {"id": "either", "kind": "signal", "subtype": "any", "config": {}},
            buy_node()
        ],
        "edges": [
            {"from": "crash", "to": "either"},
            {"from": "spike", "to": "either"},
            {"from": "either", "to": "buy"}
        ]
    });
    let input = SimulationInput {
        graph: graph(g),
        config: config("base", "1000", 3),
        policy: policy_with_signals(sig(Some("level"), None, None, None)),
        market_data: bundle("base", &["1000", "2000", "3000"], json!([]), json!([])),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "signal_fired"), 2);
}

#[test]
fn not_combinator_inverts_its_input() {
    let g = json!({
        "nodes": [
            leaf("below", "<", "1800"),
            {"id": "not-below", "kind": "signal", "subtype": "not", "config": {}},
            buy_node()
        ],
        "edges": [
            {"from": "below", "to": "not-below"},
            {"from": "not-below", "to": "buy"}
        ]
    });
    let input = SimulationInput {
        graph: graph(g),
        config: config("base", "1000", 3),
        policy: policy_with_signals(sig(Some("level"), None, None, None)),
        // not(below 1800): true, false, true
        market_data: bundle("base", &["2000", "1500", "2000"], json!([]), json!([])),
    };
    let trace = run(&input).unwrap();
    assert_eq!(count(&trace, "signal_fired"), 2);
}
