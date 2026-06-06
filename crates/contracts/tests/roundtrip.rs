//! Round-trip every shared example payload through the typed contract structs.
//!
//! The examples in `schemas/examples/` are the same fixtures the Python contract
//! package validates, so passing here keeps the two languages aligned.

use std::path::PathBuf;

use catalyst_contracts::{
    BacktestRequest, BacktestResult, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace,
};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/examples")
        .canonicalize()
        .expect("schemas/examples should exist")
}

fn read_example(name: &str) -> String {
    std::fs::read_to_string(examples_dir().join(name))
        .unwrap_or_else(|e| panic!("read {name}: {e}"))
}

/// Deserialize -> serialize -> deserialize and assert the value is stable.
fn assert_roundtrip<T>(name: &str)
where
    T: serde::de::DeserializeOwned + serde::Serialize + PartialEq + std::fmt::Debug,
{
    let raw = read_example(name);
    let parsed: T = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {name}: {e}"));
    let reserialized = serde_json::to_string(&parsed).expect("serialize");
    let reparsed: T = serde_json::from_str(&reserialized).expect("reparse");
    assert_eq!(parsed, reparsed, "round-trip changed value for {name}");
}

#[test]
fn graph_examples_roundtrip() {
    assert_roundtrip::<Graph>("graph.swap.json");
    assert_roundtrip::<Graph>("graph.perp-signal.json");
    assert_roundtrip::<Graph>("graph.yield.json");
}

#[test]
fn request_example_roundtrips() {
    assert_roundtrip::<BacktestRequest>("backtest-request.json");
}

#[test]
fn policy_example_roundtrips() {
    assert_roundtrip::<SimulationPolicy>("simulation-policy.strict_v1.json");
}

#[test]
fn market_data_example_roundtrips() {
    assert_roundtrip::<MarketDataBundle>("market-data-bundle.json");
}

#[test]
fn trace_example_roundtrips() {
    assert_roundtrip::<SimulationTrace>("simulation-trace.json");
}

#[test]
fn result_example_roundtrips() {
    assert_roundtrip::<BacktestResult>("backtest-result.json");
}

#[test]
fn graph_field_values_are_parsed() {
    let raw = read_example("graph.swap.json");
    let graph: Graph = serde_json::from_str(&raw).unwrap();
    assert_eq!(graph.nodes.len(), 1);
    let node = &graph.nodes[0];
    assert_eq!(node.id, "buy-eth-on-base");
    assert_eq!(node.config["amount"], "100");
    assert!(node.enabled);
}

#[test]
fn policy_yield_keyword_field_is_renamed() {
    let raw = read_example("simulation-policy.strict_v1.json");
    let policy: SimulationPolicy = serde_json::from_str(&raw).unwrap();
    assert_eq!(policy.profile, "strict_v1");
    let accrual = policy.yield_.as_ref().and_then(|y| y.accrual.clone());
    assert_eq!(accrual.as_deref(), Some("simple_apr"));
}
