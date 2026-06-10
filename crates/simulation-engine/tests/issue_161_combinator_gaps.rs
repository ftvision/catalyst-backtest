//! #161: combinators must not fire on missing data. Leaves yield `None` on a
//! data gap; combinators evaluate over `Option<bool>` with **Kleene
//! three-valued logic** — a missing input only makes the result missing when
//! the output actually depends on it:
//!
//! - `all(false, missing)` = `Some(false)` (determined: one input is false)
//! - `all(true,  missing)` = `None`        (the gap decides)
//! - `any(true,  missing)` = `Some(true)`  (determined: one input is true)
//! - `any(false, missing)` = `None`        (the gap decides)
//! - `not(missing)`        = `None`        (propagates)
//!
//! A `None` combinator behaves exactly like a `None` leaf in Phase 2: no fire,
//! and crossing state is frozen (the next real observation is compared against
//! the last *real* state).
//!
//! Setup: two ETH candle series. `base` is contiguous and drives the tick
//! clock; `kraken` has a hole at one tick, so a venue-pinned price leaf on
//! `kraken` yields `None` there. `missing_required = warn` keeps the interior
//! hole from aborting the run.

use std::collections::BTreeMap;

use catalyst_contracts::policy::{DataPolicy, SignalPolicy};
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

/// Two ETH series: `base` contiguous from `base_closes` (drives the ticks),
/// `kraken` only at the given `(hour, close)` points (a skipped hour = a gap).
fn two_series_bundle(base_closes: &[&str], kraken_points: &[(i64, &str)]) -> MarketDataBundle {
    let base: Vec<Value> = base_closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    let kraken: Vec<Value> = kraken_points
        .iter()
        .map(|(i, c)| json!({"ts": ts(*i), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h",
        "start": ts(0),
        "end": ts(base_closes.len() as i64),
        "candles": [
            {"venue": "base", "symbol": "ETH", "quote": "USD", "points": base},
            {"venue": "kraken", "symbol": "ETH", "quote": "USD", "points": kraken}
        ],
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

/// strict_v1 with the given trigger and `missing_required = warn` so the
/// interior hole in the kraken series doesn't abort the run.
fn policy(trigger: &str) -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None,
        fills: None,
        gas: None,
        signals: Some(SignalPolicy {
            trigger: Some(trigger.to_string()),
            repeat: None,
            cooldown: None,
            max_count: None,
        }),
        ordering: None,
        data: Some(DataPolicy {
            missing_required: Some("warn".to_string()),
            missing_optional: None,
        }),
        perps: None,
        yield_: None,
    }
}

fn graph(v: Value) -> Graph {
    serde_json::from_value(v).unwrap()
}

/// A venue-pinned price leaf, so a hole in that venue's series gaps the leaf.
fn leaf(id: &str, venue: &str, op: &str, threshold: &str) -> Value {
    json!({"id": id, "kind": "signal", "subtype": "threshold",
           "config": {"source": {"kind": "price", "symbol": "ETH", "venue": venue},
                      "operator": op, "reference": {"const": threshold}}})
}

fn buy_node() -> Value {
    json!({"id": "buy", "kind": "action", "subtype": "swap",
           "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}})
}

fn fires(trace: &SimulationTrace) -> Vec<String> {
    trace
        .events
        .iter()
        .filter(|e| e.event_type == "signal_fired")
        .map(|e| e.ts.clone())
        .collect()
}

// (1) not(gapped leaf) does not fire on the gap tick, the gap does not advance
// crossing state, and a later genuine false->true crossing fires exactly once.
#[test]
fn not_over_gapped_leaf_neither_fires_nor_advances_crossing_state() {
    // not(kraken < 1800), crossing trigger:
    //   tick 0: kraken 1500 -> below=true  -> not=false  (state false)
    //   tick 1: kraken GAP  -> not=None    -> no fire, state frozen at false
    //           (pre-fix: not(missing coerced to false) = true -> fired here)
    //   tick 2: kraken 1500 -> not=false   (still false)
    //   tick 3: kraken 2000 -> not=true    -> genuine false->true crossing: FIRE
    //   tick 4: kraken 2000 -> not=true    -> no edge
    let g = json!({
        "nodes": [
            leaf("below", "kraken", "<", "1800"),
            {"id": "not-below", "kind": "signal", "subtype": "not", "config": {}},
            buy_node()
        ],
        "edges": [
            {"from": "below", "to": "not-below"},
            {"from": "not-below", "to": "buy"}
        ]
    });
    let trace = run(&SimulationInput {
        graph: graph(g),
        config: config(5),
        policy: policy("crossing"),
        market_data: two_series_bundle(
            &["2000", "2000", "2000", "2000", "2000"],
            &[(0, "1500"), (2, "1500"), (3, "2000"), (4, "2000")],
        ),
    })
    .unwrap();
    assert_eq!(fires(&trace), vec![ts(3)], "events: {:?}", trace.events);
}

// (2) all(false, missing) = Some(false): determined despite the gap — no fire,
// and crossing state legitimately updates to false (so the next true fires).
#[test]
fn all_with_false_and_missing_is_determined_false_and_updates_state() {
    // all(base > 1800, kraken > 1800), crossing trigger:
    //   tick 0: base 2000 (true),  kraken 2000 (true)  -> all=true  -> FIRE
    //   tick 1: base 1500 (false), kraken GAP          -> all=Some(false):
    //           no fire, state updates to false
    //   tick 2: base 2000 (true),  kraken 2000 (true)  -> all=true, prev=false -> FIRE
    // (None-propagation at tick 1 would freeze state at true and suppress the
    // tick-2 crossing: only 1 fire.)
    let g = json!({
        "nodes": [
            leaf("base-hi", "base", ">", "1800"),
            leaf("kraken-hi", "kraken", ">", "1800"),
            {"id": "both", "kind": "signal", "subtype": "all", "config": {}},
            buy_node()
        ],
        "edges": [
            {"from": "base-hi", "to": "both"},
            {"from": "kraken-hi", "to": "both"},
            {"from": "both", "to": "buy"}
        ]
    });
    let trace = run(&SimulationInput {
        graph: graph(g),
        config: config(3),
        policy: policy("crossing"),
        market_data: two_series_bundle(
            &["2000", "1500", "2000"],
            &[(0, "2000"), (2, "2000")],
        ),
    })
    .unwrap();
    assert_eq!(fires(&trace), vec![ts(0), ts(2)], "events: {:?}", trace.events);
}

// (3) all(true, missing) = None: the gap decides — no fire, state frozen.
#[test]
fn all_with_true_and_missing_is_none_and_freezes_state() {
    // all(base > 1800, kraken > 1800), crossing trigger:
    //   tick 0: base true, kraken true -> all=true -> FIRE (state true)
    //   tick 1: base true, kraken GAP  -> all=None -> no fire, state frozen at true
    //           (pre-fix: gap coerced to false -> all=false -> state dropped to false)
    //   tick 2: base true, kraken true -> all=true, prev=true -> no edge, no fire
    //           (pre-fix: prev=false -> phantom second fire here)
    let g = json!({
        "nodes": [
            leaf("base-hi", "base", ">", "1800"),
            leaf("kraken-hi", "kraken", ">", "1800"),
            {"id": "both", "kind": "signal", "subtype": "all", "config": {}},
            buy_node()
        ],
        "edges": [
            {"from": "base-hi", "to": "both"},
            {"from": "kraken-hi", "to": "both"},
            {"from": "both", "to": "buy"}
        ]
    });
    let trace = run(&SimulationInput {
        graph: graph(g),
        config: config(3),
        policy: policy("crossing"),
        market_data: two_series_bundle(
            &["2000", "2000", "2000"],
            &[(0, "2000"), (2, "2000")],
        ),
    })
    .unwrap();
    assert_eq!(fires(&trace), vec![ts(0)], "events: {:?}", trace.events);
}

// (4) any(true, missing) = Some(true): determined despite the gap — FIRES.
// Guards the Kleene choice against over-strict None-propagation, which would
// silently suppress wide any/all trees whose result doesn't depend on the gap.
#[test]
fn any_with_true_and_missing_is_determined_true_and_fires() {
    // any(base > 1800, kraken < 1000), level trigger:
    //   tick 0: base 2000 (true),  kraken 1500 (false) -> any=true -> FIRE
    //   tick 1: base 2000 (true),  kraken GAP          -> any=Some(true) -> FIRE
    //   tick 2: base 1500 (false), kraken 1500 (false) -> any=false -> no fire
    let g = json!({
        "nodes": [
            leaf("base-hi", "base", ">", "1800"),
            leaf("kraken-crash", "kraken", "<", "1000"),
            {"id": "either", "kind": "signal", "subtype": "any", "config": {}},
            buy_node()
        ],
        "edges": [
            {"from": "base-hi", "to": "either"},
            {"from": "kraken-crash", "to": "either"},
            {"from": "either", "to": "buy"}
        ]
    });
    let trace = run(&SimulationInput {
        graph: graph(g),
        config: config(3),
        policy: policy("level"),
        market_data: two_series_bundle(
            &["2000", "2000", "1500"],
            &[(0, "1500"), (2, "1500")],
        ),
    })
    .unwrap();
    assert_eq!(fires(&trace), vec![ts(0), ts(1)], "events: {:?}", trace.events);
}

// (5) any(false, missing) = None: the gap decides — no fire, state frozen.
#[test]
fn any_with_false_and_missing_is_none_and_freezes_state() {
    // any(base > 1800, kraken < 1000), crossing trigger:
    //   tick 0: base true,  kraken false -> any=true  -> FIRE (state true)
    //   tick 1: base false, kraken GAP   -> any=None  -> no fire, state frozen at true
    //           (pre-fix: gap coerced to false -> any=Some(false) -> state dropped)
    //   tick 2: base true,  kraken false -> any=true, prev=true -> no edge, no fire
    //           (pre-fix: prev=false -> phantom second fire here)
    let g = json!({
        "nodes": [
            leaf("base-hi", "base", ">", "1800"),
            leaf("kraken-crash", "kraken", "<", "1000"),
            {"id": "either", "kind": "signal", "subtype": "any", "config": {}},
            buy_node()
        ],
        "edges": [
            {"from": "base-hi", "to": "either"},
            {"from": "kraken-crash", "to": "either"},
            {"from": "either", "to": "buy"}
        ]
    });
    let trace = run(&SimulationInput {
        graph: graph(g),
        config: config(3),
        policy: policy("crossing"),
        market_data: two_series_bundle(
            &["2000", "1500", "2000"],
            &[(0, "1500"), (2, "1500")],
        ),
    })
    .unwrap();
    assert_eq!(fires(&trace), vec![ts(0)], "events: {:?}", trace.events);
}

// (6) Nested: a combinator over a None combinator propagates None.
#[test]
fn nested_combinator_propagates_none() {
    // outer = all(base > 1800, not(kraken < 1800)), level trigger:
    //   tick 0: base true; kraken 1500 -> below=true  -> not=false -> all=false: no fire
    //   tick 1: base true; kraken GAP  -> not=None    -> all(true, None)=None: no fire
    //           (pre-fix: not(missing)=true -> all(true,true)=true -> fired here)
    //   tick 2: base true; kraken 2000 -> below=false -> not=true  -> all=true: FIRE
    let g = json!({
        "nodes": [
            leaf("base-hi", "base", ">", "1800"),
            leaf("below", "kraken", "<", "1800"),
            {"id": "not-below", "kind": "signal", "subtype": "not", "config": {}},
            {"id": "outer", "kind": "signal", "subtype": "all", "config": {}},
            buy_node()
        ],
        "edges": [
            {"from": "below", "to": "not-below"},
            {"from": "base-hi", "to": "outer"},
            {"from": "not-below", "to": "outer"},
            {"from": "outer", "to": "buy"}
        ]
    });
    let trace = run(&SimulationInput {
        graph: graph(g),
        config: config(3),
        policy: policy("level"),
        market_data: two_series_bundle(
            &["2000", "2000", "2000"],
            &[(0, "1500"), (2, "2000")],
        ),
    })
    .unwrap();
    assert_eq!(fires(&trace), vec![ts(2)], "events: {:?}", trace.events);
}
