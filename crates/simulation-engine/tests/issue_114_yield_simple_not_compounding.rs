//! Issue #114 (FIXED): yield accrual COMPOUNDS on `(principal + accrued)`, not
//! simple interest on `principal` alone.
//!
//! `accrue_yield` now computes `interest = (y.principal + y.accrued) * apr *
//! fraction`, so earned interest itself earns (matching real protocols such as
//! Aave aToken balances) instead of growing linearly and drifting low over long
//! horizons.
//!
//! - issue_114_accrued_compounds_on_principal_plus_accrued: the accrued value
//!   matches the per-tick-compounded figure, not the simple-interest one.
//! - issue_114_accrued_equals_per_tick_compounded: the engine accrued EQUALS an
//!   independently computed per-tick-compounding reference.
//! - issue_114_first_tick_accrual_matches_simple: at the first accrual tick
//!   (accrued still 0) compounding and simple coincide — the boundary case.

use std::collections::BTreeMap;
use std::str::FromStr;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use rust_decimal::Decimal;
use serde_json::json;

const START_EPOCH: i64 = 1_704_067_200;
const STEP: i64 = 3600;

fn ts(i: i64) -> String {
    let epoch = START_EPOCH + i * STEP;
    chrono::DateTime::from_timestamp(epoch, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// Build the standard scenario: deposit 10000 USDC into aave/usdc/base at tick 0,
/// flat APR 0.05, 1h interval, gas 0, candles 0..=n.
fn run_scenario(n: i64) -> catalyst_contracts::trace::SimulationTrace {
    let candle_points: Vec<_> = (0..=n)
        .map(|i| json!({"ts": ts(i), "open": "2000", "high": "2000", "low": "2000", "close": "2000"}))
        .collect();
    let yield_points: Vec<_> = (0..=n).map(|i| json!({"ts": ts(i), "apr": "0.05"})).collect();

    let market_data: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h",
        "start": ts(0),
        "end": ts(n),
        "candles": [{
            "venue": "base", "symbol": "ETH", "quote": "USD",
            "points": candle_points
        }],
        "gas": [{"chain": "base", "points": [{"ts": ts(0), "gas_usd": "0.0"}]}],
        "yields": [{
            "protocol": "aave", "pool": "usdc", "asset": "USDC", "chain": "base",
            "points": yield_points
        }]
    }))
    .unwrap();

    let graph: Graph = serde_json::from_value(json!({
        "nodes": [{
            "id": "deposit", "kind": "action", "subtype": "yield_deposit",
            "config": {"chain": "base", "protocol": "aave", "pool": "usdc", "asset": "USDC", "amount": "10000"}
        }],
        "edges": []
    }))
    .unwrap();

    let mut initial_portfolio: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut base = BTreeMap::new();
    base.insert("USDC".to_string(), "10000".to_string());
    initial_portfolio.insert("base".to_string(), base);

    let config = BacktestConfig {
        start: ts(0),
        end: ts(n),
        interval: "1h".to_string(),
        initial_portfolio,
        execution: None,
    };

    let policy: SimulationPolicy = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1"
    }))
    .unwrap();

    run(&SimulationInput { graph, config, policy, market_data }).unwrap()
}

/// Issue #114 (FIXED): `accrue_yield` compounds on `(principal + accrued)`. With
/// 100 accrual ticks the total accrued equals the per-tick-COMPOUNDED value
/// "5.7093754961951401003444278894", strictly larger than the old simple-interest
/// figure "5.707762557077625570776255".
#[test]
fn issue_114_accrued_compounds_on_principal_plus_accrued() {
    let trace = run_scenario(100);

    let yp = &trace.final_portfolio.yield_positions[0];

    // Principal stays as the deposited amount; the growth lives in `accrued`.
    assert_eq!(yp.principal, "10000");

    // accrue_yield runs at the START of each tick, before the tick-0 deposit, so
    // a position only exists for ticks 1..=100 => 100 accrual events.
    let accrual_events = trace
        .events
        .iter()
        .filter(|e| e.event_type == "yield_accrued")
        .count();
    assert_eq!(accrual_events, 100, "expected one accrual per tick 1..=100");

    // FIXED: compounded value (interest earns interest).
    assert_eq!(
        yp.accrued.as_deref(),
        Some("5.7093754961951401003444278894"),
        "issue #114 FIXED: compounded accrued on principal+accrued"
    );
    // And it is no longer the simple-interest value.
    assert_ne!(yp.accrued.as_deref(), Some("5.707762557077625570776255"));
}

/// Issue #114 (FIXED): the engine accrued EQUALS an independently computed
/// per-tick-compounding reference (same rust_decimal arithmetic).
#[test]
fn issue_114_accrued_equals_per_tick_compounded() {
    let trace = run_scenario(100);
    let yp = &trace.final_portfolio.yield_positions[0];

    let p = Decimal::from(10000);
    let apr = Decimal::from_str("0.05").unwrap();
    let frac = Decimal::from(3600) / Decimal::from(31_536_000);

    // Per-tick-compounded reference: interest accrues on principal + accrued.
    let mut comp = Decimal::ZERO;
    for _ in 0..100 {
        comp += (p + comp) * apr * frac;
    }

    let actual = Decimal::from_str(yp.accrued.as_deref().unwrap()).unwrap();

    assert_eq!(
        actual, comp,
        "issue #114 FIXED: engine accrued {actual} should equal compounded {comp}"
    );
    // The compounded value is strictly above the old simple-interest figure.
    let simple = p * apr * frac * Decimal::from(100);
    assert!(actual > simple, "compounded {actual} > simple {simple}");
}

/// Issue #114 edge: at the FIRST accrual tick the accrued is still 0, so the
/// compounding step `(principal + 0) * apr * frac` coincides with simple interest
/// — the boundary where the fix and the old behavior agree.
#[test]
fn issue_114_first_tick_accrual_matches_simple() {
    let trace = run_scenario(1); // deposit tick0, one accrual at tick1
    let yp = &trace.final_portfolio.yield_positions[0];

    let accrual_events =
        trace.events.iter().filter(|e| e.event_type == "yield_accrued").count();
    assert_eq!(accrual_events, 1, "exactly one accrual tick");

    let p = Decimal::from(10000);
    let apr = Decimal::from_str("0.05").unwrap();
    let frac = Decimal::from(3600) / Decimal::from(31_536_000);
    let one_tick = p * apr * frac; // == (p + 0) * apr * frac

    let actual = Decimal::from_str(yp.accrued.as_deref().unwrap()).unwrap();
    assert_eq!(actual, one_tick, "first accrual tick: compounding == simple");
}
