//! Issue #122 (DECIDED CONVENTION): same-bar fills under non-`next_open` price
//! selection are kept as an explicit "trade-on-close" convention — the same one
//! backtesting.py exposes as `trade_on_close=True` and Backtrader as
//! `cheat_on_close` — rather than deferred one bar (deferring a close-fill would
//! fill at bar N+1's close, look-ahead in the other direction). The bias is made
//! impossible to miss: every run under a non-`next_open` selection carries ONE
//! unconditional run-level warning naming the bias direction.
//!
//! Tests:
//! 1. `research_v1` (close) → trace.warnings carries the look-ahead warning.
//! 2. `strict_v1` (next_open) → NO such warning.
//! 3. SPEC pin of the convention: under `research_v1` a threshold signal computed
//!    from bar N's close fills at bar N's close + 5 bps, booked at bar N's ts.
//! 4. The warning follows the EXECUTED selection: `strict_v1` with a
//!    `fills.price_selection = "close"` policy-contract override also warns.
//!    (`config.execution`'s `ExecutionOverrides` has no `price_selection` field —
//!    `crates/contracts/src/request.rs` — so the fills section is the override path.)

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

fn policy(value: Value) -> SimulationPolicy {
    serde_json::from_value(value).unwrap()
}

/// price_threshold signal -> swap action. Fires when ETH `operator` `threshold`.
fn signal_buy_graph(threshold: &str) -> Graph {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "below", "kind": "signal", "subtype": "price_threshold",
             "config": {"symbol": "ETH", "operator": "<", "threshold": threshold}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "500", "chain": "base"}}
        ],
        "edges": [{"from": "below", "to": "buy"}]
    }))
    .unwrap()
}

fn flat_bars() -> MarketDataBundle {
    bundle(&[("2000", "2005", "1995", "2000"), ("2000", "2005", "1995", "2000")])
}

fn same_bar_warnings(trace: &SimulationTrace) -> Vec<&String> {
    trace.warnings.iter().filter(|w| w.contains("#122")).collect()
}

/// (1) A `research_v1` run (price_selection=close) unconditionally carries the
/// run-level look-ahead warning — even when no signal ever fires.
#[test]
fn research_v1_run_warns_about_same_bar_look_ahead() {
    let trace = run(&SimulationInput {
        graph: signal_buy_graph("1800"), // never true on flat 2000 bars
        config: config(2),
        policy: policy(json!({
            "schema_version": "catalyst.backtest.policy.v1",
            "profile": "research_v1"
        })),
        market_data: flat_bars(),
    })
    .unwrap();

    let hits = same_bar_warnings(&trace);
    assert_eq!(
        hits.len(),
        1,
        "research_v1 must carry exactly one #122 same-bar warning; warnings: {:?}",
        trace.warnings
    );
    assert!(
        hits[0].contains("look-ahead"),
        "the #122 warning must name the look-ahead bias; got {:?}",
        hits[0]
    );
    assert!(
        hits[0].contains("price_selection=close"),
        "the warning must name the selection that triggered it; got {:?}",
        hits[0]
    );
}

/// (2) A `strict_v1` run (price_selection=next_open) carries NO #122 warning —
/// next_open is the look-ahead-free default and must stay noise-free.
#[test]
fn strict_v1_run_has_no_same_bar_warning() {
    let trace = run(&SimulationInput {
        graph: signal_buy_graph("1800"),
        config: config(2),
        policy: policy(json!({
            "schema_version": "catalyst.backtest.policy.v1",
            "profile": "strict_v1"
        })),
        market_data: flat_bars(),
    })
    .unwrap();

    assert!(
        same_bar_warnings(&trace).is_empty(),
        "strict_v1 (next_open) must not carry a #122 same-bar warning; warnings: {:?}",
        trace.warnings
    );
    assert!(
        !trace.warnings.iter().any(|w| w.contains("look-ahead")),
        "strict_v1 must not warn about look-ahead at all; warnings: {:?}",
        trace.warnings
    );
}

/// (3) SPEC pin of the trade-on-close convention: under `research_v1` with a
/// `level` signal trigger, a threshold signal computed from bar 1's close (1700)
/// fills ON bar 1 at bar 1's close plus research_v1's 5 bps slippage:
/// 1700 * (1 + 0.0005) = 1700.85, with `action_executed` stamped at bar 1's ts.
/// This is the convention #122 decided to KEEP (warned, not deferred).
#[test]
fn research_v1_spec_signal_fills_same_bar_at_close_plus_slippage() {
    let trace = run(&SimulationInput {
        graph: signal_buy_graph("1800"),
        config: config(3),
        policy: policy(json!({
            "schema_version": "catalyst.backtest.policy.v1",
            "profile": "research_v1",
            "signals": {"trigger": "level"}
        })),
        market_data: bundle(&[
            ("2000", "2005", "1995", "2000"), // bar 0: signal false
            ("1750", "1760", "1690", "1700"), // bar 1: close 1700 < 1800 → fires
            ("1700", "1710", "1690", "1705"), // bar 2: present so bar 1 isn't the last bar
        ]),
    })
    .unwrap();

    let exec = trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed")
        .expect("the level-triggered swap must execute");

    // Booked on the DECISION bar — that's the convention being pinned.
    assert_eq!(
        exec.ts,
        ts(1),
        "trade-on-close: the fill is booked on the decision bar (bar 1), not deferred"
    );

    // Priced at the decision bar's close + 5 bps: 1700 * 1.0005 = 1700.85.
    let price: f64 = exec
        .detail
        .as_ref()
        .and_then(|d| d.get("price"))
        .and_then(|v| v.as_str())
        .expect("a fill price")
        .parse()
        .unwrap();
    assert_eq!(
        price, 1700.85,
        "trade-on-close: fill price is the signal's own observed close (1700) + 5 bps"
    );
}

/// (4) The warning follows the EXECUTED selection, not the profile name: a
/// `strict_v1` policy whose `fills.price_selection` is overridden to `close`
/// (the policy-contract override path; `config.execution` has no
/// `price_selection` field) carries the same #122 warning.
#[test]
fn strict_v1_with_close_override_carries_the_warning() {
    let trace = run(&SimulationInput {
        graph: signal_buy_graph("1800"),
        config: config(2),
        policy: policy(json!({
            "schema_version": "catalyst.backtest.policy.v1",
            "profile": "strict_v1",
            "fills": {"price_selection": "close"}
        })),
        market_data: flat_bars(),
    })
    .unwrap();

    let hits = same_bar_warnings(&trace);
    assert_eq!(
        hits.len(),
        1,
        "strict_v1 overridden to close must carry the #122 warning; warnings: {:?}",
        trace.warnings
    );
    assert!(
        hits[0].contains("look-ahead") && hits[0].contains("price_selection=close"),
        "the warning must name the executed selection and the bias; got {:?}",
        hits[0]
    );
}
