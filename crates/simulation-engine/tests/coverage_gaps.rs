//! #42: a required candle series with an interior hole fails under
//! `missing_required = fail` (strict) and warns under forward-fill (research).
//! #167: leading/trailing absence relative to the REQUESTED window (a series
//! that starts late or ends early) is enforced the same way, and the trace
//! reports the effective window (first/last actual tick) alongside the
//! requested one.

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

// --- #167: leading/trailing absence vs the REQUESTED window ---

#[test]
fn strict_fails_on_leading_absence() {
    // The series only covers the back half of the requested window: hours 2..4
    // of [0, 4]. No interior gap, but the run would silently start 2 bars late.
    let err = run(&SimulationInput {
        graph: swap_graph(),
        config: config(),
        policy: policy("strict_v1"),
        market_data: holed_bundle(&[2, 3, 4]),
    })
    .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains(&format!("cover {}..{}", ts(2), ts(4))),
        "message must state the covered window; got: {msg}"
    );
    assert!(
        msg.contains(&format!("requested window is {START}..{}", ts(4))),
        "message must state the requested window; got: {msg}"
    );
    assert!(msg.contains("2 leading / 0 trailing"), "got: {msg}");
}

#[test]
fn strict_fails_on_trailing_absence() {
    // The series ends 2 bars early: hours 0..2 of [0, 4].
    let err = run(&SimulationInput {
        graph: swap_graph(),
        config: config(),
        policy: policy("strict_v1"),
        market_data: holed_bundle(&[0, 1, 2]),
    })
    .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains(&format!("cover {START}..{}", ts(2))),
        "message must state the covered window; got: {msg}"
    );
    assert!(msg.contains("0 leading / 2 trailing"), "got: {msg}");
}

#[test]
fn strict_fails_on_empty_required_series() {
    // ZERO points in the window. Before #167 this passed silently (the interior
    // check skipped any series with fewer than two points).
    let err = run(&SimulationInput {
        graph: swap_graph(),
        config: config(),
        policy: policy("strict_v1"),
        market_data: holed_bundle(&[]),
    })
    .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains(&format!("no data in the requested window {START}..{}", ts(4))),
        "got: {msg}"
    );
}

/// Signal-gated buy: no initial actions, so the tick clock is purely
/// data-driven (an initial action would add a synthetic tick at the requested
/// start, masking the shortened effective window).
fn signal_swap_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "below", "kind": "signal", "subtype": "price_threshold",
             "config": {"symbol": "ETH", "operator": "<", "threshold": "3000"}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base"}}
        ],
        "edges": [{"from": "below", "to": "buy"}]
    }))
    .unwrap()
}

#[test]
fn research_warns_on_leading_absence_and_reports_effective_window() {
    let trace = run(&SimulationInput {
        graph: signal_swap_graph(),
        config: config(),
        policy: policy("research_v1"), // missing_required = warn-equivalent
        market_data: holed_bundle(&[2, 3, 4]),
    })
    .unwrap();
    assert!(
        trace.warnings.iter().any(|w| w.contains("2 leading / 0 trailing")),
        "warnings: {:?}",
        trace.warnings
    );
    // The trace discloses requested vs effective: the run actually began at ts(2).
    assert_eq!(trace.start, START);
    assert_eq!(trace.end, ts(4));
    assert_eq!(trace.effective_start.as_deref(), Some(ts(2).as_str()));
    assert_eq!(trace.effective_end.as_deref(), Some(ts(4).as_str()));
}

#[test]
fn research_warns_on_trailing_absence_and_reports_effective_window() {
    let trace = run(&SimulationInput {
        graph: swap_graph(),
        config: config(),
        policy: policy("research_v1"),
        market_data: holed_bundle(&[0, 1, 2]),
    })
    .unwrap();
    assert!(
        trace.warnings.iter().any(|w| w.contains("0 leading / 2 trailing")),
        "warnings: {:?}",
        trace.warnings
    );
    assert_eq!(trace.effective_start.as_deref(), Some(START));
    assert_eq!(trace.effective_end.as_deref(), Some(ts(2).as_str()));
}

#[test]
fn full_window_series_sets_effective_equal_to_requested() {
    let trace = run(&SimulationInput {
        graph: swap_graph(),
        config: config(),
        policy: policy("strict_v1"),
        market_data: holed_bundle(&[0, 1, 2, 3, 4]),
    })
    .unwrap();
    assert!(!trace.warnings.iter().any(|w| w.contains("leading")), "warnings: {:?}", trace.warnings);
    // ALWAYS set, even when nothing was shortened.
    assert_eq!(trace.effective_start.as_deref(), Some(START));
    assert_eq!(trace.effective_end.as_deref(), Some(ts(4).as_str()));
}
