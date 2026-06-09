//! Accrual must scale with the *actual* elapsed time between ticks, not a fixed
//! `interval_secs`. The tick clock is data-driven (candle/series timestamps) and
//! can have gaps — e.g. the gaps our candle cleaning leaves behind. A position
//! held across a gap must accrue the whole elapsed interval, not one tick's worth.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const EPOCH: i64 = 1_704_067_200; // 2024-01-01T00:00:00Z

fn iso(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Aave USDC yield series at 0h, 1h, **3h** (a missing 2h candle = a tick gap),
/// flat 5% APR. No candles needed — the yield series drives the tick clock.
fn bundle() -> MarketDataBundle {
    let yields: Vec<Value> = [0, 1, 3]
        .iter()
        .map(|h| json!({"ts": iso(EPOCH + h * 3600), "apr": "0.05"}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": iso(EPOCH), "end": iso(EPOCH + 3 * 3600),
        "candles": [], "funding": [], "gas": [],
        "yields": [{"protocol": "aave", "asset": "USDC", "chain": "base", "pool": "usdc", "points": yields}],
        "providers": [], "warnings": []
    }))
    .unwrap()
}

fn config() -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), "11000".to_string());
    let mut init = BTreeMap::new();
    init.insert("base".to_string(), bal);
    BacktestConfig {
        start: iso(EPOCH),
        end: iso(EPOCH + 3 * 3600),
        interval: "1h".to_string(),
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

/// Deposit a fixed 10,000 USDC into Aave on the first tick (an initial action).
fn deposit_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "deposit", "kind": "action", "subtype": "yield_deposit",
            "config": {"chain": "base", "protocol": "aave", "asset": "USDC",
                       "pool": "usdc", "amount": "10000"}}],
        "edges": []
    }))
    .unwrap()
}

fn accrued_total(t: &SimulationTrace) -> f64 {
    t.events
        .iter()
        .filter(|e| e.event_type == "yield_accrued")
        .map(|e| {
            e.detail.as_ref().unwrap()["interest_usd"].as_str().unwrap().parse::<f64>().unwrap()
        })
        .sum()
}

#[test]
fn yield_accrues_full_elapsed_time_across_a_tick_gap() {
    let trace = run(&SimulationInput {
        graph: deposit_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle(),
    })
    .unwrap();

    // Deposited 10,000 at 5% APR, held from 0h to 3h = 3 hours. Accrual fires at
    // tick 1 (elapsed 1h since deposit) and tick 3 (elapsed 2h across the gap),
    // for 3h total elapsed — the key #134 property (the gap hour is NOT dropped;
    // the pre-fix static-interval accrual charged only 1h at tick 3 = 2h total).
    // Interest compounds (#114): tick 3 accrues on principal + tick-1 interest.
    let total = accrued_total(&trace);
    let (p, apr, yr) = (10000.0, 0.05, 31_536_000.0);
    let i1 = p * apr * (3600.0 / yr); // tick 1: 1h on principal
    let i2 = (p + i1) * apr * (2.0 * 3600.0 / yr); // tick 3: 2h on principal + accrued
    let expected_3h = i1 + i2;
    let buggy_2h = p * apr * (2.0 * 3600.0 / yr); // static-interval bug (gap hour dropped)
    assert!(
        (total - expected_3h).abs() < 1e-9,
        "expected ~{expected_3h:.6} (3h elapsed, compounded), got {total:.6}; buggy value is {buggy_2h:.6}"
    );
}
