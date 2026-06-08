//! Issue #118 (+ companion #113) — accrual must use the ACTUAL elapsed time
//! between ticks, not a STATIC `interval_secs` window, so a tick that follows a
//! data gap accrues the full elapsed interval (and sums every funding point that
//! lands in the gap).
//!
//! STATUS: CONFIRMS #118/#113 FIXED by PR #134 — regression guard.
//! `crates/simulation-engine/src/engine.rs` now derives
//! `elapsed_secs = ts - prev_ts` (engine.rs:241) and feeds it to both
//! `accrue_funding` (window `(ts - elapsed_secs, ts]`, engine.rs:988) and
//! `accrue_yield` (`fraction = elapsed_secs / YEAR_SECONDS`, engine.rs:1028).
//! These tests PIN the correct elapsed-time values; if the static-interval bug
//! ever regresses, the gap-tick assertions below fail.
//!
//! Per-test verdict (both FIXED):
//!   * `issue_113_yield_accrues_full_elapsed_across_a_one_bar_gap` — yield
//!     accrual at the gap tick charges the full 2h elapsed (NOT one 1h slice).
//!   * `issue_118_funding_window_sums_all_points_inside_a_gap` — the funding
//!     window at the gap tick spans (1h, 3h] and sums BOTH the in-gap 2h point
//!     and the 3h point (rate 0.002), not just the last interval's point.
//!
//! Complements (does not duplicate):
//!   * `accrual_gaps.rs` — checks the run-total yield across a yield-driven tick
//!     clock with no candles; here we assert the per-event gap-tick value with a
//!     candle-driven clock.
//!   * `funding_interval.rs` — checks summing within a coarse (4h) bar with no
//!     gap; here we assert summing across an actual one-bar data gap.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const EPOCH: i64 = 1_704_067_200; // 2024-01-01T00:00:00Z

fn iso(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// strict_v1 default aborts on interior data gaps (missing_required=Fail).
/// forward_fill downgrades that abort to a warning WITHOUT injecting synthetic
/// ticks — the tick grid is still just the candle timestamps, so the gap (and
/// thus the elapsed-time discrepancy) is preserved.
fn gap_tolerant_policy() -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "data": {"missing_required": "forward_fill"},
        "gas": {"model": "none"}
    }))
    .unwrap()
}

fn events_of<'a>(t: &'a SimulationTrace, kind: &str) -> Vec<&'a catalyst_contracts::trace::Event> {
    t.events.iter().filter(|e| e.event_type == kind).collect()
}

// ---------------------------------------------------------------------------
// #113 — yield accrual across a one-bar gap
// ---------------------------------------------------------------------------

/// ISSUE #113 (FIXED by PR #134) — yield accrual scales by the ACTUAL elapsed
/// time since the previous tick, so the tick AFTER a data gap accrues the full
/// elapsed interval rather than a single static `interval_secs` slice.
///
/// Bundle: base/ETH candles at 0h, 1h, 3h (the 2h candle is MISSING -> a
/// one-bar gap), flat close 1000. yields aave/USDC/base apr=0.10 (forward
/// filled). 1000 USDC deposited into aave (initial action at tick 0); accrual
/// fires at ticks 1h and 3h.
///
/// The 3h tick is 2h after the previous (1h) tick, so it accrues 2h of
/// interest: 1000 * 0.10 * 7200/31_536_000 = "0.02283105022831050228310502"
/// (NOT one 1h slice "0.01141552511415525114155251"). This regression guard
/// pins the correct 2h figure; if the static-interval bug returns, the gap-tick
/// assertion below fails.
#[test]
fn issue_113_yield_accrues_full_elapsed_across_a_one_bar_gap() {
    // candles at 0h, 1h, 3h (2h missing)
    let candle_pts: Vec<Value> = [0i64, 1, 3]
        .iter()
        .map(|h| json!({"ts": iso(EPOCH + h * 3600), "open": "1000", "high": "1000", "low": "1000", "close": "1000"}))
        .collect();

    let bundle: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": iso(EPOCH), "end": iso(EPOCH + 3 * 3600),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": candle_pts}],
        "funding": [],
        "gas": [],
        "yields": [{"protocol": "aave", "asset": "USDC", "chain": "base",
                    "points": [{"ts": iso(EPOCH), "apr": "0.10"}]}],
        "providers": [], "warnings": []
    }))
    .unwrap();

    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), "1000".to_string());
    let mut init = BTreeMap::new();
    init.insert("base".to_string(), bal);
    let config = BacktestConfig {
        start: iso(EPOCH),
        end: iso(EPOCH + 3 * 3600),
        interval: "1h".to_string(),
        initial_portfolio: init,
        execution: None,
    };

    let graph: Graph = serde_json::from_value(json!({
        "nodes": [{"id": "dep", "kind": "action", "subtype": "yield_deposit",
            "config": {"chain": "base", "protocol": "aave", "asset": "USDC", "amount": "1000"}}],
        "edges": []
    }))
    .unwrap();

    let trace = run(&SimulationInput { graph, config, policy: gap_tolerant_policy(), market_data: bundle }).unwrap();

    let ye = events_of(&trace, "yield_accrued");
    assert_eq!(ye.len(), 2, "expected accrual at ticks 1h and 3h");

    // Correct/expected magnitudes.
    const ONE_HOUR: &str = "0.01141552511415525114155251"; // 1000*0.10*3600/31536000
    const TWO_HOUR: &str = "0.02283105022831050228310502"; // 1000*0.10*7200/31536000 (correct gap accrual)

    // Tick 1h: a normal 1h slice.
    assert_eq!(ye[0].ts, iso(EPOCH + 3600));
    let i0 = ye[0].detail.as_ref().unwrap()["interest_usd"].as_str().unwrap();
    assert_eq!(i0, ONE_HOUR, "first (non-gap) tick accrues exactly one 1h slice");

    // Tick 3h (the gap tick): 2h elapsed since the 1h tick -> the full 2h figure.
    assert_eq!(ye[1].ts, iso(EPOCH + 3 * 3600));
    let i1 = ye[1].detail.as_ref().unwrap()["interest_usd"].as_str().unwrap();
    assert_eq!(
        i1, TWO_HOUR,
        "ISSUE #113 FIXED: gap tick (3h, 2h elapsed) accrues the full 2h ({TWO_HOUR}), not 1h ({ONE_HOUR})"
    );
    assert_ne!(
        i1, ONE_HOUR,
        "ISSUE #113 regression: gap tick must NOT under-accrue to a single 1h slice"
    );

    // Whole-run interest is the correct 1h + 2h elapsed total (0.0342...),
    // not the old under-accrued 2x1h figure (0.0228...).
    let sum: f64 = i0.parse::<f64>().unwrap() + i1.parse::<f64>().unwrap();
    let under_accrued = 0.022_831_050_228_310_502_f64; // old bug: 2 x one-hour slices
    let correct_total = 0.034_246_575_342_465_753_f64; // 1h + 2h
    assert!(
        (sum - correct_total).abs() < 1e-9,
        "ISSUE #113 FIXED: total interest {sum} matches the correct 1h+2h {correct_total}, not {under_accrued}"
    );
    assert!(sum > under_accrued, "ISSUE #113: total interest covers the full elapsed gap");
}

// ---------------------------------------------------------------------------
// #118 — funding lookback window skips a point inside a gap
// ---------------------------------------------------------------------------

/// ISSUE #118 (FIXED by PR #134) — funding accrual sums the ACTUAL inter-tick
/// window `(ts - elapsed_secs, ts]` (engine.rs:988) rather than a fixed
/// `interval_secs` lookback, so a funding point that falls inside a data gap is
/// still summed.
///
/// Bundle: hyperliquid/ETH candles at 0h, 1h, 3h (2h candle MISSING -> gap),
/// flat close 2000. funding hourly rate 0.001 at 1h, 2h, 3h — the 2h point
/// lands INSIDE the gap. A 1000 USD long ETH (leverage 1) is opened at tick 0;
/// fill = 2000 + 10bps = 2002, size = 1000/2002, marked at 2000 ->
/// notional = (1000/2002)*2000 = 999.000999000999000999000999.
///
/// At tick 3h, 2h elapsed since the 1h tick, so the window is (1h, 3h] and sums
/// BOTH the in-gap 2h point AND the 3h point: rate "0.002", payment ~1.998 (NOT
/// the single 3h point rate "0.001", ~0.999). This regression guard pins the
/// two-point sum; if the static window returns, the gap-tick assertion fails.
#[test]
fn issue_118_funding_window_sums_all_points_inside_a_gap() {
    let candle_pts: Vec<Value> = [0i64, 1, 3]
        .iter()
        .map(|h| json!({"ts": iso(EPOCH + h * 3600), "open": "2000", "high": "2000", "low": "2000", "close": "2000"}))
        .collect();
    // funding at 1h, 2h, 3h — the 2h point is inside the missing-candle gap.
    let funding_pts: Vec<Value> = [1i64, 2, 3]
        .iter()
        .map(|h| json!({"ts": iso(EPOCH + h * 3600), "rate": "0.001"}))
        .collect();

    let bundle: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": iso(EPOCH), "end": iso(EPOCH + 3 * 3600),
        "candles": [{"venue": "hyperliquid", "symbol": "ETH", "quote": "USD", "points": candle_pts}],
        "funding": [{"venue": "hyperliquid", "symbol": "ETH", "points": funding_pts}],
        "gas": [],
        "yields": [],
        "providers": [], "warnings": []
    }))
    .unwrap();

    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), "2000".to_string());
    let mut init = BTreeMap::new();
    init.insert("hyperliquid".to_string(), bal);
    let config = BacktestConfig {
        start: iso(EPOCH),
        end: iso(EPOCH + 3 * 3600),
        interval: "1h".to_string(),
        initial_portfolio: init,
        execution: None,
    };

    let graph: Graph = serde_json::from_value(json!({
        "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
            "config": {"symbol": "ETH", "side": "long", "size_usd": "1000", "leverage": "1",
                       "chain": "hyperliquid", "order_type": "market", "reduce_only": false}}],
        "edges": []
    }))
    .unwrap();

    let trace = run(&SimulationInput { graph, config, policy: gap_tolerant_policy(), market_data: bundle }).unwrap();

    let fe = events_of(&trace, "funding_applied");
    assert_eq!(fe.len(), 2, "expected funding at ticks 1h and 3h");

    // Tick 1h: a single 1h point.
    assert_eq!(fe[0].ts, iso(EPOCH + 3600));
    assert_eq!(fe[0].detail.as_ref().unwrap()["rate"].as_str().unwrap(), "0.001");

    // Tick 3h (gap tick): window (1h, 3h] sums the in-gap 2h point AND the 3h point.
    assert_eq!(fe[1].ts, iso(EPOCH + 3 * 3600));
    let rate = fe[1].detail.as_ref().unwrap()["rate"].as_str().unwrap();
    assert_eq!(
        rate, "0.002",
        "ISSUE #118 FIXED: gap tick (3h) sums the 2h+3h points (rate 0.002), not just the 3h point (0.001)"
    );
    assert_ne!(
        rate, "0.001",
        "ISSUE #118 regression: gap tick must NOT skip the in-gap 2h funding point"
    );

    let pay: f64 = fe[1].detail.as_ref().unwrap()["payment_usd"].as_str().unwrap().parse().unwrap();
    assert!(
        (pay - 1.998_001_998).abs() < 1e-6,
        "ISSUE #118 FIXED: gap-tick payment {pay} matches the two-point ~1.998, not the single-point ~0.999"
    );
    assert!(
        (pay - 0.999_000_999).abs() > 1e-3,
        "ISSUE #118 regression: payment must NOT collapse to the single-point ~0.999"
    );
}
