//! Per-run execution overrides (`BacktestConfig.execution`) now take effect:
//! a run can change the signal trigger without defining a new policy profile.
//! This is what turns the g19 funding carry into a clean open-once-and-hold.

use std::path::PathBuf;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";

fn ts(i: i64) -> String {
    chrono::DateTime::from_timestamp(1_704_067_200 + i * 3600, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn bundle(venue: &str, closes: &[&str], funding: Value) -> MarketDataBundle {
    let pts: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    let gas: Vec<_> =
        closes.iter().enumerate().map(|(i, _)| json!({"ts": ts(i as i64), "gas_usd": "0.0"})).collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(closes.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": pts}],
        "funding": funding, "gas": [{"chain": venue, "points": gas}], "yields": [],
        "providers": [], "warnings": []
    }))
    .unwrap()
}

fn config(venue: &str, usdc: &str, n: i64, trigger: Option<&str>) -> BacktestConfig {
    let execution = trigger.map(|t| json!({ "signal_trigger": t }));
    serde_json::from_value(json!({
        "start": START, "end": ts(n), "interval": "1h",
        "initial_portfolio": { venue: { "USDC": usdc } },
        "execution": execution,
    }))
    .unwrap()
}

fn strict() -> SimulationPolicy {
    serde_json::from_value(json!({"schema_version": "catalyst.backtest.policy.v1", "profile": "strict_v1"}))
        .unwrap()
}

fn count(t: &catalyst_contracts::SimulationTrace, k: &str) -> usize {
    t.events.iter().filter(|e| e.event_type == k).count()
}

fn catalog_graph(file: &str) -> Graph {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../strategies/graphs")
        .join(file)
        .canonicalize()
        .unwrap();
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn buy_below_1800() -> Graph {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "below", "kind": "signal", "subtype": "price_threshold",
             "config": {"symbol": "ETH", "operator": "<", "threshold": "1800"}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}}
        ],
        "edges": [{"from": "below", "to": "buy"}]
    }))
    .unwrap()
}

#[test]
fn signal_trigger_override_changes_firing() {
    // dips below 1800 twice -> strict (crossing) fires twice.
    let md = bundle("base", &["1700", "2000", "1700"], json!([]));
    let base = run(&SimulationInput {
        graph: buy_below_1800(),
        config: config("base", "1000", 3, None),
        policy: strict(),
        market_data: md.clone(),
    })
    .unwrap();
    assert_eq!(count(&base, "signal_fired"), 2, "crossing fires on each dip");

    // once_per_backtest via the run's execution override -> fires once.
    let once = run(&SimulationInput {
        graph: buy_below_1800(),
        config: config("base", "1000", 3, Some("once_per_backtest")),
        policy: strict(),
        market_data: md,
    })
    .unwrap();
    assert_eq!(count(&once, "signal_fired"), 1, "override limits it to one fire");
}

#[test]
fn g19_with_once_per_backtest_opens_the_basis_and_holds() {
    let venue = "hyperliquid";
    // funding stays rich (>= 0.00001) the whole run.
    let funding = json!([{"venue": venue, "symbol": "ETH", "points":
        (0..6).map(|i| json!({"ts": ts(i), "rate": "0.0003"})).collect::<Vec<_>>()}]);
    let trace = run(&SimulationInput {
        graph: catalog_graph("g19_funding_carry.json"),
        config: config(venue, "10000", 6, Some("once_per_backtest")),
        policy: strict(),
        market_data: bundle(venue, &["2000"; 6], funding),
    })
    .unwrap();
    // opens the long-spot + short-perp legs exactly once, then holds — no
    // re-opening, no rejections.
    assert_eq!(count(&trace, "action_executed"), 2, "open both legs once");
    assert_eq!(count(&trace, "action_rejected"), 0, "no balance-exhausting re-opens");
}
