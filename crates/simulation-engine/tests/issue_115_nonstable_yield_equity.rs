//! Issue #115 (FIXED) — non-stable yield positions are marked to price, not 1:1 USD.
//!
//!   * `issue_115_nonstable_yield_marked_to_price` (Bug #1): `compute_equity` now
//!     values a non-stable yield position as `y.value() * mark_price` (like the
//!     spot branch), so 1 ETH in a vault is worth ~$1800, not $1. Depositing it is
//!     value-neutral — the equity change across the deposit reflects only the ETH
//!     price, not a phantom collapse to $1.
//!
//!   * `issue_115_yield_deposit_value_usd_is_usd_notional` (Bug #3):
//!     `execute_yield_deposit` reports `Fill.value_usd = amount * price`, so a
//!     1 ETH deposit at price 1800 records value_usd = 1800 (the USD notional).

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

/// Bug #1 — FIXED: a non-stable yield position is marked to price, not 1:1 USD.
/// `compute_equity` now values a non-stable yield position as `y.value() * mark`,
/// like the spot branch.
///
/// closes [2000, 1800, 1800]; the deposit (1 ETH) executes at tick1. The yield
/// ETH is marked at each tick's price, so equity = 1 * close: 2000, 1800, 1800.
/// The deposit itself is value-neutral (1 ETH spot @1800 == 1 ETH vault @1800);
/// the 2000 -> 1800 step is the ETH price move, NOT a collapse to $1.
#[test]
fn issue_115_nonstable_yield_marked_to_price() {
    let trace = run(&input()).unwrap();
    assert_eq!(trace.snapshots.len(), 3);

    let e0 = &trace.snapshots[0].equity_usd;
    let e1 = &trace.snapshots[1].equity_usd;
    let e2 = &trace.snapshots[2].equity_usd;

    assert_eq!(e0, "2000", "pre-deposit: 1 ETH spot marked 1 * 2000");
    assert_eq!(
        e1, "1800",
        "FIXED #115: 1 ETH in the vault is marked at close_1 (1800), not counted as $1"
    );
    assert_eq!(e2, "1800", "FIXED #115: still 1 ETH in the vault @1800");

    // Value-neutral across the deposit: the vault ETH is worth exactly what the
    // spot ETH was at tick1 (1 * 1800), i.e. no phantom collapse to $1.
    assert_ne!(e1, "1", "FIXED #115: equity is no longer collapsed to asset units");
}

/// Bug #3 — FIXED: `execute_yield_deposit` reports `Fill.value_usd = amount * price`.
///
/// A 1 ETH deposit at the tick1 price (1800) records value_usd = "1800" (the USD
/// notional), while `amount` stays in asset units ("1"). gas_usd is "0" (gas model
/// "none").
#[test]
fn issue_115_yield_deposit_value_usd_is_usd_notional() {
    let trace = run(&input()).unwrap();

    let evt = trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed")
        .expect("yield_deposit should execute once");
    let d = evt.detail.as_ref().expect("action_executed event has a detail object");

    assert_eq!(d["kind"], "yield_deposit");
    assert_eq!(d["side"], "deposit");
    assert_eq!(d["amount"], "1"); // amount stays in asset units

    assert_eq!(
        d["value_usd"], "1800",
        "FIXED #115: value_usd = amount * price = 1 ETH * 1800 USD"
    );
    assert_eq!(d["gas_usd"], "0", "gas model 'none' -> zero gas");
}
