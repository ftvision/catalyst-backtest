//! Issue #115 — non-stable yield position is valued 1:1 as USD, not marked to price.
//!
//! Both tests below record bugs that are STILL PRESENT in the current engine:
//!
//!   * `issue_115_nonstable_yield_deposit_collapses_equity` (Bug #1): when a
//!     non-stable asset (ETH) is deposited into a yield position,
//!     `compute_equity` adds `y.value()` (asset units) directly to equity
//!     WITHOUT multiplying by the mark price (unlike the spot-balance branch).
//!     So 1 ETH worth ~$2000 is counted as $1 and equity collapses 2000 -> 1
//!     across the deposit (a discontinuity).
//!
//!   * `issue_115_yield_deposit_fill_value_usd_is_asset_units` (Bug #3):
//!     `execute_yield_deposit` sets `Fill.value_usd = Some(amount)` (asset
//!     units) instead of `amount * price`, so the action_executed trade record
//!     reports value_usd=1 for a 1 ETH deposit instead of the USD notional 1800.
//!
//! Each test PINS the current (incorrect) value so it PASSES today, and
//! documents the CORRECT value a fixed engine should produce.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::json;

const START: &str = "2024-01-01T00:00:00Z";
const START_EPOCH: i64 = 1_704_067_200;
const STEP: i64 = 3600;

fn ts(i: i64) -> String {
    let epoch = START_EPOCH + i * STEP;
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Flat-ETH bundle on chain `base` over 3 hourly ticks; closes drive ETH price.
/// No funding/gas/yields series -> zero interest accrues; yield value stays at
/// the deposited principal (1 ETH) exactly.
fn bundle(closes: &[&str]) -> MarketDataBundle {
    let points: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h",
        "start": ts(0),
        "end": ts(3),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [],
        "gas": [],
        "yields": [],
        "providers": [],
        "warnings": []
    }))
    .unwrap()
}

/// Initial portfolio = 1 ETH on chain `base` (NON-stable asset — the crux).
fn config() -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("ETH".to_string(), "1".to_string());
    let mut init = BTreeMap::new();
    init.insert("base".to_string(), bal);
    BacktestConfig {
        start: START.to_string(),
        end: ts(3),
        interval: "1h".to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

/// strict_v1 with gas model "none" (gas_usd=0, isolates Bug #1 from gas Bug #2)
/// and level-trigger signals.
fn policy() -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "gas": {"model": "none"},
        "signals": {"trigger": "level"}
    }))
    .unwrap()
}

/// price_threshold(ETH < 1900) -> yield_deposit(base/aave/ETH, amount 1).
fn graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "dip", "kind": "signal", "subtype": "price_threshold",
             "config": {"symbol": "ETH", "operator": "<", "threshold": "1900"}},
            {"id": "dep", "kind": "action", "subtype": "yield_deposit",
             "config": {"chain": "base", "protocol": "aave", "asset": "ETH", "amount": "1"}}
        ],
        "edges": [{"from": "dip", "to": "dep"}]
    }))
    .unwrap()
}

fn input() -> SimulationInput {
    // closes: tick0 ETH=2000 (>=1900, no fire); tick1 & tick2 ETH=1800 (<1900,
    // signal fires). Deposit executes once at tick1 (1 ETH); at tick2 the ETH
    // spot balance is 0 so the second deposit is rejected -> one deposit event.
    SimulationInput {
        graph: graph(),
        config: config(),
        policy: policy(),
        market_data: bundle(&["2000", "1800", "1800"]),
    }
}

/// Bug #1 — non-stable yield principal is valued 1:1 as USD instead of marked
/// to price. The engine's `compute_equity` does `equity += y.value()` for yield
/// positions (no `is_stable` check, no `* mark_price`), unlike the spot branch
/// which does `equity += amt * price`.
///
/// ISSUE #115: the engine reports equity = "1" at tick1 (1 ETH deposited),
/// which is WRONG. The CORRECT value is "2000" — 1 ETH marked at its ~$2000
/// price — so equity should stay CONTINUOUS across the deposit (gas is 0).
#[test]
fn issue_115_nonstable_yield_deposit_collapses_equity() {
    let trace = run(&input()).unwrap();
    assert_eq!(trace.snapshots.len(), 3);

    let e0 = &trace.snapshots[0].equity_usd;
    let e1 = &trace.snapshots[1].equity_usd;
    let e2 = &trace.snapshots[2].equity_usd;

    // Pre-deposit: 1 ETH spot, correctly marked 1 * 2000.
    assert_eq!(e0, "2000", "pre-deposit ETH spot marked to USD");

    // PIN THE BUG: after depositing 1 ETH into yield, equity collapses to "1"
    // (asset units counted as USD). CORRECT would be "2000".
    assert_eq!(
        e1, "1",
        "BUG #115: 1 ETH in a yield position is counted as $1, not ~$2000 (mark price ignored)"
    );
    assert_eq!(e2, "1", "BUG #115: collapse persists at tick2 (still in yield)");

    // Make the defect explicit: the equity is discontinuous across the deposit.
    // A FIXED engine would mark the yield principal to price, giving e1 == e0.
    assert_ne!(
        e1, e0,
        "BUG #115 witness: equity is discontinuous across the deposit; once fixed e1 should equal e0 (\"2000\")"
    );
}

/// Bug #3 — `execute_yield_deposit` sets `Fill.value_usd = Some(amount)`
/// (asset units) rather than `amount * price`.
///
/// ISSUE #115: the action_executed trade record reports value_usd = "1" for a
/// 1 ETH deposit, which is WRONG. The CORRECT value is "1800" — the USD notional
/// amount * ETH price at the deposit tick (1 * 1800). We also confirm gas_usd is
/// "0" so the equity-collapse test (Bug #1) is isolated from the gas bug.
#[test]
fn issue_115_yield_deposit_fill_value_usd_is_asset_units() {
    let trace = run(&input()).unwrap();

    let evt = trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed")
        .expect("yield_deposit should execute once");
    let d = evt.detail.as_ref().expect("action_executed event has a detail object");

    assert_eq!(d["kind"], "yield_deposit");
    assert_eq!(d["side"], "deposit");
    assert_eq!(d["amount"], "1");

    // PIN THE BUG: value_usd reports asset units (1), not USD notional (1800).
    assert_eq!(
        d["value_usd"], "1",
        "BUG #115: yield_deposit Fill.value_usd reports asset units (1) not USD (amount * price = 1800)"
    );
    assert_ne!(
        d["value_usd"], "1800",
        "BUG #115 witness: a fixed engine would report the USD notional \"1800\""
    );

    // gas model "none" zeroes gas, isolating Bug #1 from the gas-units bug.
    assert_eq!(d["gas_usd"], "0", "gas excluded so equity-collapse test isolates Bug #1");
}
