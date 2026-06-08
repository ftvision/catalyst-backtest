//! Issue #116: with `next_open` (strict_v1) fills, the position and cash debit are
//! booked at the DECISION bar (bar 0) instead of the FILL bar (bar 1). This injects
//! a phantom entry-bar P&L into the equity curve because the just-bought ETH is marked
//! at close_0 even though it was acquired at open_1.
//!
//! Verdict per test (mode: demonstrate, bug PRESENT in current code):
//! - issue_116_entry_bar_books_position_one_bar_early: PRESENT. Pins the buggy bar-0
//!   ledger (ETH already present, USDC already debited). Correct: no fill on bar 0.
//! - issue_116_entry_bar_equity_has_phantom_pnl: PRESENT. Pins the buggy bar-0 equity
//!   (~975.21). Correct: 1000 (untouched cash).
//! - issue_116_fill_bar_equity_is_correct_reference: ANCHOR. Bar-1 numbers and the fill
//!   price are already correct; this passes before and after the fix.

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

/// Issue #116 (PRESENT): the swap decided on bar 0 fills at bar 1's open (2102.1),
/// yet the engine books the ETH position and the cash debit into the bar-0 snapshot.
///
/// The values asserted below are the CURRENT (INCORRECT) behavior. Correct behavior:
/// on the decision bar no fill has occurred, so balances["base"] should have no "ETH"
/// (or ETH == 0) and USDC should remain at the full initial 1000. The position and
/// debit must appear only on snapshots[1] (the fill bar).
#[test]
fn issue_116_entry_bar_books_position_one_bar_early() {
    let trace = run_buy();
    let b0 = trace.snapshots[0].portfolio.as_ref().unwrap();
    let base0 = &b0.balances["base"];

    // BUG: ETH already present on the decision bar (should be absent / zero).
    assert_eq!(
        base0.get("ETH").map(|d| d.to_string()).as_deref(),
        Some("0.2378573807145235716664288093"),
        "issue #116: ETH position booked on bar 0 (decision bar) is INCORRECT; \
         on bar 0 no fill has occurred so ETH should be absent/zero, the position \
         belongs on bar 1 (the fill bar)"
    );
    // BUG: cash already debited (500 notional + 0.25 fee + 0.25 gas) on the decision bar.
    assert_eq!(
        base0["USDC"].to_string(),
        "499.5",
        "issue #116: USDC debited on bar 0 is INCORRECT; it should still be the full \
         initial 1000 until the fill on bar 1"
    );
    // Make the discrepancy explicit: this is NOT the correct deferred-booking value.
    assert_ne!(
        base0["USDC"].to_string(),
        "1000",
        "issue #116: bar-0 USDC differs from the correct untouched 1000"
    );
}

/// Issue #116 (PRESENT): phantom entry-bar P&L. Bar-0 equity marks the just-bought ETH
/// at close_0 = 2000 even though it was acquired at open_1 = 2102.1, injecting a
/// fictitious (2000 - 2102.1) * 0.2378573807... = -24.2852 loss into the equity curve.
///
/// The asserted value is CURRENT (INCORRECT). Correct bar-0 equity = 1000 (the unspent
/// initial cash, with no ETH and no fees/gas booked yet).
#[test]
fn issue_116_entry_bar_equity_has_phantom_pnl() {
    let trace = run_buy();

    // BUG: bar-0 equity = 0.2378573807...*2000 (mark at close_0) + 499.5 USDC.
    assert_eq!(
        trace.snapshots[0].equity_usd.to_string(),
        "975.2147614290471433328576186",
        "issue #116: bar-0 equity ~975.21 is INCORRECT; it carries a phantom \
         -24.2852 loss from marking unfilled ETH at close_0. Correct bar-0 equity \
         is 1000 (unspent initial cash)"
    );
    // Make the discrepancy explicit against the correct deferred value.
    assert_ne!(
        trace.snapshots[0].equity_usd.to_string(),
        "1000",
        "issue #116: bar-0 equity differs from the correct 1000 by the phantom entry-bar P&L"
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
