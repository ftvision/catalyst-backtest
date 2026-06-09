//! Issue #116 (FIXED): with `next_open` (strict_v1) fills, a market order decided on
//! bar N is now DEFERRED and booked at the FILL bar (bar N+1's open), not the
//! decision bar. This removes the phantom entry-bar P&L that arose from marking the
//! just-bought ETH at close_N even though it was acquired at open_{N+1}.
//!
//! Verdict per test (mode: regression guard, bug FIXED in current code):
//! - issue_116_entry_bar_books_position_one_bar_early: bar-0 ledger is untouched (no
//!   ETH, USDC == 1000); the position/debit land on bar 1.
//! - issue_116_entry_bar_equity_has_phantom_pnl: bar-0 equity == 1000 (no phantom mark).
//! - issue_116_fill_bar_equity_is_correct_reference: ANCHOR. Bar-1 numbers and the fill
//!   price were always correct; this passes before and after the fix.
//!
//! SPEC — correct execution-timing semantics (now PASSING):
//! The three `issue_116_spec_*` tests assert that an action decided on bar N is booked
//! at bar N+1's OPEN, and that a signal firing on the LAST bar does NOT execute in
//! strict mode (no next bar to fill against without look-ahead). These were the
//! red-by-default spec before the fix; they are green now that next_open fills are
//! deferred to the fill bar.

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

fn run_buy() -> SimulationTrace {
    run(&SimulationInput {
        graph: buy_graph(),
        config: config(2),
        policy: strict(),
        market_data: bundle(&[
            ("2000", "2005", "1995", "2000"),
            ("2100", "2110", "2090", "2105"),
        ]),
    })
    .unwrap()
}

/// Issue #116 (FIXED — regression guard): the swap decided on bar 0 fills at bar 1's
/// open (2102.1), and the engine now defers the booking to the fill bar. On the
/// decision bar (bar 0) no fill has occurred, so `balances["base"]` holds no `ETH`
/// and `USDC` is the full initial 1000. The position and debit appear only on
/// snapshots[1] (the fill bar). Before the #116 fix this bar-0 snapshot wrongly
/// carried ETH == 0.2378573807… and USDC == 499.5.
#[test]
fn issue_116_entry_bar_books_position_one_bar_early() {
    let trace = run_buy();
    let b0 = trace.snapshots[0].portfolio.as_ref().unwrap();
    let base0 = &b0.balances["base"];

    // FIXED: no ETH on the decision bar — the fill is deferred to bar 1's open.
    assert_eq!(
        base0.get("ETH").map(|d| d.to_string()),
        None,
        "issue #116: bar-0 (decision bar) must hold no ETH — the position belongs on \
         the fill bar (bar 1). Pre-fix the engine wrongly booked it on bar 0."
    );
    // FIXED: cash untouched on the decision bar — nothing is debited until bar 1.
    assert_eq!(
        base0["USDC"].to_string(),
        "1000",
        "issue #116: bar-0 (decision bar) USDC must be the full initial 1000 — the \
         500 notional + 0.25 fee + 0.25 gas debit lands on the fill bar (bar 1). \
         Pre-fix the engine wrongly debited to 499.5 on bar 0."
    );
}

/// Issue #116 (FIXED — regression guard): no phantom entry-bar P&L. With the fill
/// deferred to bar 1, the bar-0 equity is exactly the unspent initial cash (1000) —
/// no ETH is marked, no fees/gas are booked. Before the fix, bar-0 equity was
/// ~975.21 because the just-"bought" ETH was marked at close_0 = 2000 even though it
/// was acquired at open_1 = 2102.1, injecting a fictitious
/// (2000 - 2102.1) * 0.2378573807… = -24.2852 loss into the equity curve.
#[test]
fn issue_116_entry_bar_equity_has_phantom_pnl() {
    let trace = run_buy();

    // FIXED: bar-0 equity = the untouched initial cash, no phantom mark.
    assert_eq!(
        trace.snapshots[0].equity_usd.to_string(),
        "1000",
        "issue #116: bar-0 equity must be 1000 (unspent initial cash). Pre-fix it was \
         ~975.21, carrying a phantom -24.2852 loss from marking unfilled ETH at close_0."
    );
}

/// Issue #116 ANCHOR (PRESENT/correct): the fill PRICE is right (#41) and bar 1 is the
/// real fill bar. This documents that only the BOOKING TIME is wrong, not the price or
/// the bar-1 numbers. This test should keep passing both before and after the fix.
#[test]
fn issue_116_fill_bar_equity_is_correct_reference() {
    let trace = run_buy();

    // Fill price = bar1 open 2100 * (1 + 10bps) = 2102.1 (correct per #41).
    let price = trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed")
        .and_then(|e| e.detail.as_ref())
        .and_then(|d| d.get("price"))
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(price, "2102.100", "issue #116 anchor: fill price is correct (#41)");

    // Bar-1 (fill bar) equity: 0.2378573807...*2105 (mark at close_1) + 499.5 USDC.
    assert_eq!(
        trace.snapshots[1].equity_usd.to_string(),
        "1000.1897864040721183578326436",
        "issue #116 anchor: bar-1 (fill bar) equity is the correct post-fill value"
    );

    // The ETH position correctly lives on bar 1.
    let b1 = trace.snapshots[1].portfolio.as_ref().unwrap();
    assert_eq!(
        b1.balances["base"]["ETH"].to_string(),
        "0.2378573807145235716664288093",
        "issue #116 anchor: ETH position belongs on the fill bar (bar 1)"
    );
}

// ---------------------------------------------------------------------------
// SPEC (fail by default) — correct post-fix behavior. These FAIL on the current
// engine on purpose: the red is the visible record of bug #116.
// ---------------------------------------------------------------------------

const SPEC_IGNORE: &str = "spec for #116; passes once next_open fills are deferred to the fill bar";

/// Timestamp helper for an arbitrary interval (seconds per bar).
fn ts_step(i: i64, step_secs: i64) -> String {
    chrono::DateTime::from_timestamp(START_EPOCH + i * step_secs, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Like `bundle`, but parametrized by interval label + bar spacing in seconds.
fn bundle_interval(
    interval: &str,
    step_secs: i64,
    bars: &[(&str, &str, &str, &str)],
) -> MarketDataBundle {
    let points: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| {
            json!({"ts": ts_step(i as i64, step_secs), "open": o, "high": h, "low": l, "close": c})
        })
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": interval,
        "start": ts_step(0, step_secs),
        "end": ts_step(bars.len() as i64, step_secs),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

/// Like `config`, but parametrized by interval label + bar spacing in seconds.
fn config_interval(interval: &str, step_secs: i64, n_ticks: i64) -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), "1000".to_string());
    let mut init = BTreeMap::new();
    init.insert("base".to_string(), bal);
    BacktestConfig {
        start: ts_step(0, step_secs),
        end: ts_step(n_ticks, step_secs),
        interval: interval.to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

/// price_threshold signal -> swap action. Signal fires when ETH crosses below `threshold`.
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

fn count_events(trace: &SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

/// Parse a decimal-string balance (contracts represent `Decimal` as `String`) to f64.
/// A missing key is treated as zero.
fn bal(balances: &BTreeMap<String, String>, asset: &str) -> f64 {
    balances
        .get(asset)
        .map(|s| s.parse::<f64>().unwrap_or_else(|_| panic!("non-numeric balance {s:?}")))
        .unwrap_or(0.0)
}

/// SPEC #1 (CORRECT, currently FAILS): a signal that becomes true only on the LAST
/// bar must NOT execute in strict mode — there is no next bar to fill against, and
/// filling at the final close would be intra-bar look-ahead.
///
/// 3-bar 1h ETH path: 2000, 2000, 1700. The `< 1800` threshold is true only on the
/// final candle, so `signal_fired` fires at the last ts. The CORRECT post-fix engine
/// then suppresses execution: zero `action_executed`, and the final snapshot ledger is
/// untouched (USDC == 1000, no ETH key).
///
/// Current engine WRONGLY fills it at close_M (look-ahead), so this FAILS today on the
/// "action did not execute" assertions.
#[test]
fn issue_116_spec_end_of_horizon_signal_action_not_executed_strict() {
    let _ = SPEC_IGNORE;
    let trace = run(&SimulationInput {
        graph: signal_buy_graph("1800"),
        config: config_interval("1h", 3600, 3),
        policy: strict(),
        market_data: bundle_interval(
            "1h",
            3600,
            &[
                ("2000", "2005", "1995", "2000"),
                ("2000", "2005", "1995", "2000"),
                ("1700", "1705", "1695", "1700"),
            ],
        ),
    })
    .unwrap();

    let last_ts = ts(2);

    // The signal DID fire on the last bar.
    let fired_last = trace
        .events
        .iter()
        .any(|e| e.event_type == "signal_fired" && e.ts == last_ts);
    assert!(
        fired_last,
        "issue #116 spec: the price_threshold signal must fire on the last bar ({last_ts}); \
         events: {:?}",
        trace.events
    );

    // CORRECT: the action must NOT execute — no next bar to fill against in strict mode.
    assert_eq!(
        count_events(&trace, "action_executed"),
        0,
        "issue #116 spec: a signal firing on the LAST bar must NOT execute in strict mode \
         (no next bar to fill at without look-ahead). The current engine WRONGLY fills it at \
         the final close (close_M look-ahead)."
    );

    // CORRECT: the ledger is untouched in the final snapshot — full cash, no ETH.
    let last = trace.snapshots.last().unwrap();
    let base = &last.portfolio.as_ref().unwrap().balances["base"];
    assert_eq!(
        base["USDC"].to_string(),
        "1000",
        "issue #116 spec: USDC must remain the full initial 1000 — the last-bar signal must \
         not execute, so no debit. The current engine wrongly debits cash from a close_M fill."
    );
    assert_eq!(
        bal(base, "ETH"),
        0.0,
        "issue #116 spec: no ETH should be acquired from a last-bar signal in strict mode; \
         got ETH = {:?}",
        base.get("ETH")
    );
}

/// SPEC #2 (CORRECT, currently FAILS): an action's effect must land on the NEXT tick.
/// An initial swap (no incoming edge) decided on bar 0 of a 2-bar 1h run fills at bar 1's
/// OPEN, so the bar-0 snapshot must be UNTOUCHED (USDC == 1000, no ETH) and the bar-1
/// snapshot must carry the position (ETH present, USDC debited). The `action_executed`
/// event ts must equal bar 1's timestamp.
///
/// Current engine books the fill on bar 0 (the #116 bug), so this FAILS today on the
/// bar-0 "ledger untouched" assertions.
#[test]
fn issue_116_spec_action_executes_at_next_tick_1h() {
    let _ = SPEC_IGNORE;
    let trace = run(&SimulationInput {
        graph: buy_graph(),
        config: config_interval("1h", 3600, 2),
        policy: strict(),
        market_data: bundle_interval(
            "1h",
            3600,
            &[("2000", "2005", "1995", "2000"), ("2100", "2110", "2090", "2105")],
        ),
    })
    .unwrap();

    // CORRECT: decision bar (bar 0) ledger is untouched.
    let b0 = &trace.snapshots[0].portfolio.as_ref().unwrap().balances["base"];
    assert_eq!(
        b0["USDC"].to_string(),
        "1000",
        "issue #116 spec: bar-0 (decision bar) USDC must be the full initial 1000 — the fill \
         is deferred to bar 1's open. The current engine wrongly debits cash on bar 0."
    );
    assert_eq!(
        bal(b0, "ETH"),
        0.0,
        "issue #116 spec: bar-0 (decision bar) must hold no ETH — the position belongs on the \
         fill bar (bar 1). The current engine wrongly books ETH on bar 0; got {:?}",
        b0.get("ETH")
    );

    // CORRECT: the next tick (bar 1) carries the position.
    let b1 = &trace.snapshots[1].portfolio.as_ref().unwrap().balances["base"];
    assert!(
        bal(b1, "ETH") > 0.0,
        "issue #116 spec: bar-1 (fill bar) must carry the ETH position; got {:?}",
        b1.get("ETH")
    );
    assert!(
        bal(b1, "USDC") < 1000.0,
        "issue #116 spec: bar-1 (fill bar) USDC must be debited; got {}",
        b1["USDC"]
    );

    // CORRECT: the fill event is timestamped at bar 1.
    let exec = trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed")
        .expect("an action_executed event");
    assert_eq!(
        exec.ts,
        ts(1),
        "issue #116 spec: the action_executed event must be timestamped at bar 1's tick \
         ({}); the current engine emits it on the decision bar (bar 0).",
        ts(1)
    );
}

/// SPEC #3 (CORRECT, currently FAILS): same as #2 but on a 4h interval with candles
/// spaced 4h apart. The effect must land on the NEXT 4h tick, not the decision tick.
///
/// Current engine books the fill on the decision tick (the #116 bug), so this FAILS
/// today on the decision-tick "ledger untouched" assertions.
#[test]
fn issue_116_spec_action_executes_at_next_tick_4h() {
    let _ = SPEC_IGNORE;
    let step = 4 * 3600;
    let trace = run(&SimulationInput {
        graph: buy_graph(),
        config: config_interval("4h", step, 2),
        policy: strict(),
        market_data: bundle_interval(
            "4h",
            step,
            &[("2000", "2005", "1995", "2000"), ("2100", "2110", "2090", "2105")],
        ),
    })
    .unwrap();

    // CORRECT: decision tick (bar 0) ledger untouched.
    let b0 = &trace.snapshots[0].portfolio.as_ref().unwrap().balances["base"];
    assert_eq!(
        b0["USDC"].to_string(),
        "1000",
        "issue #116 spec (4h): decision-tick USDC must be the full initial 1000 — the fill is \
         deferred to the next 4h tick. The current engine wrongly debits on the decision tick."
    );
    assert_eq!(
        bal(b0, "ETH"),
        0.0,
        "issue #116 spec (4h): decision-tick must hold no ETH — the position belongs on the \
         next 4h tick. The current engine wrongly books ETH on the decision tick; got {:?}",
        b0.get("ETH")
    );

    // CORRECT: next 4h tick (bar 1) carries the position.
    let b1 = &trace.snapshots[1].portfolio.as_ref().unwrap().balances["base"];
    assert!(
        bal(b1, "ETH") > 0.0,
        "issue #116 spec (4h): the next 4h tick must carry the ETH position; got {:?}",
        b1.get("ETH")
    );
    assert!(
        bal(b1, "USDC") < 1000.0,
        "issue #116 spec (4h): the next 4h tick USDC must be debited; got {}",
        b1["USDC"]
    );

    // CORRECT: the fill event is timestamped at the next 4h tick.
    let exec = trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed")
        .expect("an action_executed event");
    assert_eq!(
        exec.ts,
        ts_step(1, step),
        "issue #116 spec (4h): the action_executed event must be timestamped at the next 4h \
         tick ({}); the current engine emits it on the decision tick.",
        ts_step(1, step)
    );
}
