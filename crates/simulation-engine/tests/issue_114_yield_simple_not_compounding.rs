//! Issue #114: yield accrual uses SIMPLE interest, not compounding.
//!
//! BUG LOCATION: crates/simulation-engine/src/engine.rs:1024 in `accrue_yield`:
//!   `let interest = y.principal * apr * fraction;`
//! Interest is computed on `principal` only (a constant), so each tick adds the
//! same amount and the position grows linearly (simple interest). A correct
//! engine compounds on `(principal + accrued)`.
//!
//! VERDICT: bug is PRESENT. Both tests below pass by pinning the engine's
//! current (incorrect, simple-interest) accrued value and documenting the
//! correct (compounding) value it should produce instead.
//!
//! - issue_114_accrued_is_simple_interest: pins the exact simple-interest
//!   accrued string the engine currently produces.
//! - issue_114_accrued_strictly_less_than_compounded: shows the engine's
//!   accrued is strictly below the per-tick-compounded reference.

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

/// Issue #114 (PRESENT): `accrue_yield` uses `y.principal` (constant) instead of
/// `(y.principal + y.accrued)`. With 100 accrual ticks the total accrued equals
/// the SIMPLE-interest value `100 * 10000 * 0.05 * (3600/31536000)` =
/// "5.707762557077625570776255".
///
/// This is INCORRECT. A compounding engine would report
/// "5.7093754961951401003444278894" (strictly larger). The exact-string
/// assertion below pins the buggy behavior; a correct fix would change it.
#[test]
fn issue_114_accrued_is_simple_interest() {
    let trace = run_scenario(100);

    let yp = &trace.final_portfolio.yield_positions[0];

    // Principal is never folded into the position (part of the same bug).
    assert_eq!(yp.principal, "10000");

    // accrue_yield runs at the START of each tick, before the tick-0 deposit, so
    // a position only exists for ticks 1..=100 => 100 accrual events.
    let accrual_events = trace
        .events
        .iter()
        .filter(|e| e.event_type == "yield_accrued")
        .count();
    assert_eq!(accrual_events, 100, "expected one accrual per tick 1..=100");

    // INCORRECT (simple interest) value the engine currently produces.
    assert_eq!(
        yp.accrued.as_deref(),
        Some("5.707762557077625570776255"),
        "issue #114: engine produces simple-interest accrued; correct compounding \
         value would be 5.7093754961951401003444278894"
    );

    // Make the discrepancy explicit: the engine value is NOT the compounding value.
    assert_ne!(
        yp.accrued.as_deref(),
        Some("5.7093754961951401003444278894"),
        "issue #114: engine should (but does not) compound on principal+accrued"
    );
}

/// Issue #114 (PRESENT): simple interest under-accrues versus per-tick
/// compounding. The engine's accrued is STRICTLY LESS than the compounded
/// reference computed here with the same rust_decimal arithmetic. A correct
/// engine would report a value EQUAL to `comp`, not strictly less.
#[test]
fn issue_114_accrued_strictly_less_than_compounded() {
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

    // The engine under-accrues (simple < compounded) -- the issue #114 defect.
    assert!(
        actual < comp,
        "issue #114: engine accrued {} should equal compounded {} but is strictly less (simple interest)",
        actual,
        comp
    );

    // Pin both endpoints for clarity.
    assert_eq!(actual.normalize().to_string(), "5.707762557077625570776255");
    assert_eq!(comp.normalize().to_string(), "5.7093754961951401003444278894");
}
