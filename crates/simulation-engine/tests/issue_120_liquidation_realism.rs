//! Issue #120 — liquidation realism. Both sub-bugs are PRESENT in the current engine.
//!
//! The engine's `check_liquidations` marks each perp at the bar CLOSE only
//! (`mark_price` in engine.rs returns `bar.close`) and liquidates ONLY when
//! `unrealized_pnl(mark) <= -margin` — i.e. full bankruptcy, with no maintenance
//! buffer. Two consequences are recorded here:
//!
//!   (a) `issue_120_long_escapes_liquidation_via_intrabar_wick`
//!       A long that goes deeply underwater on the bar LOW (wick) but recovers by
//!       the CLOSE is NOT liquidated, because only the close is used to mark.
//!       PRESENT: position survives (no "liquidation" event, ETH perp remains).
//!
//!   (b) `issue_120_long_survives_right_up_to_full_bankruptcy_no_maintenance_buffer`
//!       A long whose close-marked pnl is -99.9001 (only 0.0999 of 100 margin left)
//!       still survives, because the trigger is full bankruptcy (pnl <= -margin),
//!       with no maintenance-margin buffer.
//!       PRESENT: position survives (no "liquidation" event, ETH perp remains).
//!
//! Each test asserts the CURRENT (incorrect) behavior so it PASSES on buggy code,
//! and would FAIL once intrabar-extreme marking / a maintenance-margin model is
//! implemented. See per-test docs for the exact pnl numbers.

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

/// Issue #120 sub-bug (a): CLOSE-ONLY MARKING.
///
/// tick1 has LOW=1700 but CLOSE=2000. Entry price 2002, size_base 0.4995004995.
///   - At the bar LOW: unrealized_pnl = (1700 - 2002) * 0.4995004995 = -150.85 USD,
///     which is <= -100 (margin) => a wick-aware engine WOULD liquidate at tick1.
///   - At the bar CLOSE: unrealized_pnl = (2000 - 2002) * 0.4995004995 = -0.999 USD,
///     which is > -100 => the current close-only engine does NOT liquidate.
///   (liquidation threshold mark for this position is 1801.8: pnl <= -100.)
///
/// CURRENT (INCORRECT) behavior recorded below: no "liquidation" event, ETH perp
/// survives. CORRECT behavior (issue #120): liquidate at the wick (LOW=1700).
#[test]
fn issue_120_long_escapes_liquidation_via_intrabar_wick() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "1700", "2000"), // deep wick to 1700, recovers to close 2000
            ("2000", "2000", "2000", "2000"),
        ]),
    })
    .unwrap();

    assert!(
        trace.events.iter().all(|e| e.event_type != "liquidation"),
        "issue #120(a): engine emitted NO liquidation, marking only the bar close (2000, \
         pnl -0.999) and ignoring the intrabar LOW (1700, pnl -150.85 <= -100 margin). \
         A wick-aware engine SHOULD liquidate at tick1."
    );
    assert!(
        trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "issue #120(a): ETH long survived (INCORRECT). It should have been liquidated by \
         the bar LOW of 1700 (pnl -150.85 <= -100 margin) and removed from final_portfolio."
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
