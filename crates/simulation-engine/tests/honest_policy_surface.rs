//! The policy surface is honest (#157, #164; epic #131 Tier 0b):
//!
//! - The trace's `policy` echoes EVERY executed knob — including per-run
//!   execution overrides — and re-resolving it reproduces the executed policy.
//!   Result metadata can never report assumptions the run didn't use.
//! - The `yield.accrual` knob is wired, not decorative: `compound_apy`
//!   (default), `simple_apr`, and `none` produce three different accruals.
//! - Unimplemented policy values fail the run loudly at policy resolution.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
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

/// A 3-tick yield scenario: deposit 10000 USDC at tick 0, flat 5% APR, zero gas.
fn input_with_policy(policy: SimulationPolicy) -> SimulationInput {
    let n = 3i64;
    let candle_points: Vec<_> = (0..=n)
        .map(|i| json!({"ts": ts(i), "open": "2000", "high": "2000", "low": "2000", "close": "2000"}))
        .collect();
    let yield_points: Vec<_> = (0..=n).map(|i| json!({"ts": ts(i), "apr": "0.05"})).collect();
    let market_data: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(n),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": candle_points}],
        "gas": [{"chain": "base", "points": [{"ts": ts(0), "gas_usd": "0.0"}]}],
        "yields": [{"protocol": "aave", "pool": "usdc", "asset": "USDC", "chain": "base",
                    "points": yield_points}]
    }))
    .unwrap();
    let graph: Graph = serde_json::from_value(json!({
        "nodes": [{"id": "deposit", "kind": "action", "subtype": "yield_deposit",
            "config": {"chain": "base", "protocol": "aave", "pool": "usdc",
                       "asset": "USDC", "amount": "10000"}}],
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
    SimulationInput { graph, config, policy, market_data }
}

fn strict_with(extra: serde_json::Value) -> SimulationPolicy {
    let mut v = json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1"
    });
    v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    serde_json::from_value(v).unwrap()
}

fn accrued(trace: &catalyst_contracts::trace::SimulationTrace) -> f64 {
    trace.final_portfolio.yield_positions[0]
        .accrued
        .as_ref()
        .map(|d| d.to_string().parse().unwrap())
        .unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// yield.accrual is wired: three variants, three behaviors (#164).
// ---------------------------------------------------------------------------

#[test]
fn yield_accrual_variants_differentiate() {
    let compound = run(&input_with_policy(strict_with(json!({})))).unwrap();
    let simple =
        run(&input_with_policy(strict_with(json!({"yield": {"accrual": "simple_apr"}})))).unwrap();
    let off = run(&input_with_policy(strict_with(json!({"yield": {"accrual": "none"}})))).unwrap();

    let (c, s, o) = (accrued(&compound), accrued(&simple), accrued(&off));

    // 3 hourly accrual slices on 10000 at 5% APR.
    let per_tick: f64 = 0.05 * 3600.0 / 31_536_000.0;
    let expected_simple = 10000.0 * per_tick * 3.0;
    let expected_compound = 10000.0 * ((1.0 + per_tick).powi(3) - 1.0);

    assert!((s - expected_simple).abs() < 1e-9, "simple_apr: got {s}, want {expected_simple}");
    assert!(
        (c - expected_compound).abs() < 1e-9,
        "compound_apy (default): got {c}, want {expected_compound}"
    );
    assert!(c > s, "compounding must exceed simple interest");
    assert_eq!(o, 0.0, "accrual='none' is the off-switch: zero interest");
    assert_eq!(
        off.events.iter().filter(|e| e.event_type == "yield_accrued").count(),
        0,
        "accrual='none' emits no yield_accrued events"
    );
}

// ---------------------------------------------------------------------------
// #157: the trace echoes the EXECUTED policy, overrides included.
// ---------------------------------------------------------------------------

#[test]
fn trace_policy_echoes_executed_knobs_including_overrides() {
    let mut input = input_with_policy(strict_with(json!({})));
    input.config.execution = serde_json::from_value(json!({
        "slippage_bps": "77",
        "signal_trigger": "level"
    }))
    .unwrap();
    let trace = run(&input).unwrap();

    let fills = trace.policy.fills.as_ref().expect("#157: fills section echoed");
    assert_eq!(
        fills.slippage.as_ref().unwrap().bps.as_deref(),
        Some("77"),
        "#157: the per-run slippage override must appear in the trace policy"
    );
    let signals = trace.policy.signals.as_ref().expect("#157: signals section echoed");
    assert_eq!(
        signals.trigger.as_deref(),
        Some("level"),
        "#157: the per-run trigger override must appear in the trace policy"
    );
    // The untouched knobs echo the profile values.
    assert_eq!(fills.price_selection.as_deref(), Some("next_open"));
    assert_eq!(
        trace.policy.yield_.as_ref().unwrap().accrual.as_deref(),
        Some("compound_apy"),
        "#157: the executed accrual model is reported, not a stale default"
    );
}

// ---------------------------------------------------------------------------
// Implement-or-reject: a run with an unimplemented policy value fails loudly.
// ---------------------------------------------------------------------------

#[test]
fn run_rejects_unimplemented_policy_values() {
    let err = run(&input_with_policy(strict_with(
        json!({"ordering": {"same_tick": "conservative_adverse_order"}}),
    )));
    let msg = format!("{}", err.expect_err("inert same_tick variant must fail the run"));
    assert!(
        msg.contains("not implemented") && msg.contains("#141"),
        "the error names the unimplemented value and its tracking issue; got {msg}"
    );
}
