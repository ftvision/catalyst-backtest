//! Issue #120 — liquidation realism.
//!
//! Sub-bug (a) — CLOSE-ONLY MARKING — is now FIXED: `check_liquidations` marks
//! each perp at the worst price it touches *within* the bar (a long at the bar
//! LOW, a short at the bar HIGH), so a position that breaches its margin intrabar
//! can no longer escape just because it recovers by the close. Sub-bug (b) — no
//! MAINTENANCE-MARGIN buffer (liquidation only at full bankruptcy) — remains a
//! tracked fidelity enhancement and is NOT addressed here.
//!
//!   (a) FIXED — regression guards:
//!       - `issue_120_long_liquidated_on_intrabar_wick`
//!       - `issue_120_short_liquidated_on_intrabar_wick`
//!       - `issue_120_no_liquidation_when_wick_does_not_breach_margin`
//!   (b) PRESENT (fidelity gap): `issue_120_long_survives_right_up_to_full_bankruptcy_no_maintenance_buffer`
//!
//! See per-test docs for the exact pnl numbers.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const EPOCH: i64 = 1_704_067_200;

fn iso(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Build a 3-bar (4h) ETH market with the given (open, high, low, close) per tick.
fn bundle(bars: [(&str, &str, &str, &str); 3]) -> MarketDataBundle {
    let candles: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| {
            json!({"ts": iso(EPOCH + (i as i64) * 4 * 3600), "open": o, "high": h, "low": l, "close": c})
        })
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "4h", "start": iso(EPOCH), "end": iso(EPOCH + 8 * 3600),
        "candles": [{"venue": "hyperliquid", "symbol": "ETH", "quote": "USD", "points": candles}],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
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

/// Open a 10x long ETH perp at tick0. strict_v1 => NextOpen fill at tick1.open*1.001.
/// size_usd=1000, leverage=10 => margin_usd=100, entry_price=2002, size_base=0.4995004995...
fn open_long_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
            "config": {"symbol": "ETH", "side": "long", "size_usd": "1000", "leverage": "10",
                       "chain": "hyperliquid", "order_type": "market", "reduce_only": false}}],
        "edges": []
    }))
    .unwrap()
}

/// Open a 10x SHORT ETH perp at tick0. strict_v1 => NextOpen fill at tick1.open*0.999
/// (sell slippage). size_usd=1000, leverage=10 => margin_usd=100, entry_price=1998,
/// size_base=1000/1998=0.5005005005...
fn open_short_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
            "config": {"symbol": "ETH", "side": "short", "size_usd": "1000", "leverage": "10",
                       "chain": "hyperliquid", "order_type": "market", "reduce_only": false}}],
        "edges": []
    }))
    .unwrap()
}

/// Issue #120 sub-bug (a) — FIXED: a long is liquidated on the intrabar LOW even
/// when the bar recovers by its CLOSE.
///
/// The position is open by tick1 (NextOpen fill, entry 2002, size 0.4995004995,
/// margin 100). The wick is on the LAST bar (tick2) — where the position exists
/// regardless of whether next_open fills are booked on the decision bar (current)
/// or deferred to the fill bar (#116). At tick2 LOW=1700: pnl = (1700-2002)*0.4995
/// = -150.85 <= -100 margin => LIQUIDATE, even though CLOSE=2000 (pnl -0.999) would
/// not. Close-only marking (the old bug) would have let it escape.
#[test]
fn issue_120_long_liquidated_on_intrabar_wick() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"), // open fills here (NextOpen)
            ("2000", "2000", "1700", "2000"), // wick to 1700, recovers to close 2000
        ]),
    })
    .unwrap();

    assert!(
        trace.events.iter().any(|e| e.event_type == "liquidation"),
        "issue #120(a) FIXED: the long must be liquidated on the bar LOW (1700, pnl \
         -150.85 <= -100 margin) even though the close (2000) recovers."
    );
    assert!(
        !trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "issue #120(a) FIXED: the liquidated ETH long must be gone from final_portfolio."
    );
}

/// Issue #120 sub-bug (a) — FIXED, SHORT side: a short is liquidated on the
/// intrabar HIGH even when the bar recovers by its CLOSE.
///
/// Short entry 1998 (sell slippage), size 0.5005005005, margin 100. At tick2
/// HIGH=2300: pnl = (1998-2300)*0.5005 = -151.15 <= -100 => LIQUIDATE, though the
/// close (2000, pnl -1.001) would not.
#[test]
fn issue_120_short_liquidated_on_intrabar_wick() {
    let trace = run(&SimulationInput {
        graph: open_short_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"), // open fills here (NextOpen)
            ("2000", "2300", "2000", "2000"), // spikes to 2300, recovers to close 2000
        ]),
    })
    .unwrap();

    assert!(
        trace.events.iter().any(|e| e.event_type == "liquidation"),
        "issue #120(a) FIXED (short): liquidate on the bar HIGH (2300, pnl -151.15 <= -100)."
    );
    assert!(
        !trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "issue #120(a) FIXED (short): the liquidated ETH short must be gone."
    );
}

/// Issue #120 — guard against OVER-liquidation: a wick that does NOT breach the
/// margin must leave the position open. Long entry 2002, margin 100; tick2 LOW=1900
/// gives pnl = (1900-2002)*0.4995 = -50.95 > -100 => NO liquidation.
#[test]
fn issue_120_no_liquidation_when_wick_does_not_breach_margin() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "1900", "2000"), // shallow wick: pnl -50.95, within margin
        ]),
    })
    .unwrap();

    assert!(
        trace.events.iter().all(|e| e.event_type != "liquidation"),
        "issue #120: a wick to 1900 (pnl -50.95 > -100 margin) must NOT liquidate."
    );
    assert!(
        trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "issue #120: the ETH long must survive a non-breaching wick."
    );
}

/// Issue #120 sub-bug (b): NO MAINTENANCE-MARGIN BUFFER (liquidation only at full bankruptcy).
///
/// tick1 has LOW=CLOSE=1802 (no wick beyond close, isolating this bug from (a)).
/// Entry 2002, size_base 0.4995004995, margin_usd 100.
///   - At the close mark 1802: unrealized_pnl = (1802 - 2002) * 0.4995004995 = -99.9001 USD.
///     Only 0.0999 of the 100 margin remains, yet -99.9001 > -100, so the current
///     full-bankruptcy trigger (pnl <= -margin, i.e. mark <= 1801.8) does NOT fire.
///
/// CURRENT (INCORRECT) behavior recorded below: no "liquidation" event, ETH perp
/// survives. CORRECT behavior (issue #120): any positive maintenance buffer
/// liquidates well before bankruptcy, so this near-margin-floor position should be
/// liquidated at tick1 (with a small residual margin settled back).
#[test]
fn issue_120_long_survives_right_up_to_full_bankruptcy_no_maintenance_buffer() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "1802", "1802"), // close 1802 => pnl -99.9001, just inside -margin
            ("1802", "1802", "1802", "1802"),
        ]),
    })
    .unwrap();

    assert!(
        trace.events.iter().all(|e| e.event_type != "liquidation"),
        "issue #120(b): engine emitted NO liquidation. close-marked pnl = -99.9001 (only \
         0.0999 of 100 margin left), but the trigger is full bankruptcy (pnl <= -100, \
         mark <= 1801.8). A maintenance-margin model SHOULD liquidate here."
    );
    assert!(
        trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "issue #120(b): ETH long survived right up to the margin floor (INCORRECT). Under a \
         maintenance-margin model (pnl -99.9001 vs margin 100) it should have been liquidated."
    );
}
