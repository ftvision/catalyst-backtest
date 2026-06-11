//! Issue #160: malformed config input is rejected loudly at the layer that
//! owns it, never silently coerced to a default.
//!
//! Engine-owned site: `initial_portfolio` amounts. A typo'd starting balance
//! (`"10,000"`, `"1e4"`) used to `unwrap_or(ZERO)` — silently starting the run
//! broke, with every downstream number valid-looking but wrong. Now the run
//! fails at startup naming the offending venue/asset/string.
//!
//! The other #160 sites are tested where they live: time_in_force and relative
//! sizing values in `crates/graph-compiler/tests/compiler.rs` (compile time),
//! cooldown in `crates/simulation-policies/tests/policies.rs` (validate time),
//! and the `resolve_amount` runtime backstop as a unit test in `engine.rs`
//! (unreachable through `run()` since the compiler rejects first).

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const START_EPOCH: i64 = 1_704_067_200;
const STEP: i64 = 3600;

fn ts(i: i64) -> String {
    let epoch = START_EPOCH + i * STEP;
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn bundle(venue: &str, closes: &[&str]) -> MarketDataBundle {
    let points: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(closes.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

fn config(initial: Value, n_ticks: i64) -> BacktestConfig {
    serde_json::from_value(json!({
        "start": START,
        // Last bar is ts(n_ticks - 1); #167 enforces the window matches the data.
        "end": ts(n_ticks - 1),
        "interval": "1h",
        "initial_portfolio": initial,
    }))
    .unwrap()
}

fn strict() -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1"
    }))
    .unwrap()
}

fn swap_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base"}
        }],
        "edges": []
    }))
    .unwrap()
}

fn input_with_initial(initial: Value) -> SimulationInput {
    SimulationInput {
        graph: swap_graph(),
        config: config(initial, 2),
        policy: strict(),
        market_data: bundle("base", &["2000", "2000"]),
    }
}

#[test]
fn malformed_initial_portfolio_amount_fails_the_run() {
    let err = run(&input_with_initial(json!({"base": {"USDC": "10,000"}}))).unwrap_err();
    assert_eq!(
        err.to_string(),
        "config error: initial_portfolio: amount \"10,000\" for base/USDC is not a valid decimal"
    );
}

#[test]
fn negative_initial_portfolio_amount_fails_the_run() {
    let err = run(&input_with_initial(json!({"base": {"USDC": "-500"}}))).unwrap_err();
    assert_eq!(
        err.to_string(),
        "config error: initial_portfolio: amount \"-500\" for base/USDC must be non-negative"
    );
}

#[test]
fn valid_initial_portfolio_still_runs() {
    // Guard against over-rejecting: a plain decimal (and zero) still starts.
    run(&input_with_initial(json!({"base": {"USDC": "1000", "ETH": "0"}}))).unwrap();
}
