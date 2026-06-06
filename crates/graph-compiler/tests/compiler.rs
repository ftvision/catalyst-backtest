//! Compiler tests over the shared sample graphs (parity with the Python compiler).

use std::collections::HashMap;
use std::path::PathBuf;

use catalyst_contracts::Graph;
use catalyst_graph_compiler::{compile, TriggerType};
use serde_json::Value;

fn sample_graphs() -> HashMap<String, Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/sample_graphs.json")
        .canonicalize()
        .unwrap();
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn graph(value: &Value) -> Graph {
    serde_json::from_value(value.clone()).unwrap()
}

#[test]
fn all_sample_graphs_compile_with_triggers() {
    let graphs = sample_graphs();
    assert_eq!(graphs.len(), 15);
    for (name, g) in &graphs {
        let compiled = compile(&graph(g)).unwrap_or_else(|e| panic!("[{name}] {e}"));
        for action in &compiled.actions {
            assert!(!action.triggers.is_empty(), "[{name}] action {} has no triggers", action.id);
        }
    }
}

#[test]
fn single_swap_is_initial() {
    let g = graph(&sample_graphs()["g01_evm_swap_buy_eth_base"]);
    let c = compile(&g).unwrap();
    assert_eq!(c.actions.len(), 1);
    assert!(c.signals.is_empty());
    assert_eq!(c.actions[0].triggers[0].trigger_type, TriggerType::Initial);
}

#[test]
fn signal_drives_action() {
    let c = compile(&graph(&sample_graphs()["g11_evm_swap_if_below"])).unwrap();
    assert_eq!(c.signals.len(), 1);
    assert_eq!(c.signals[0].targets, vec!["buy-eth-on-base"]);
    let buy = &c.actions[0];
    assert_eq!(buy.triggers[0].trigger_type, TriggerType::Signal);
    assert_eq!(buy.triggers[0].source_id.as_deref(), Some("eth-below-1800"));
}

#[test]
fn action_chains_to_action() {
    let c = compile(&graph(&sample_graphs()["g03_hl_spot_buy_then_sell"])).unwrap();
    let sell = c.actions.iter().find(|a| a.id == "sell-eth-spot").unwrap();
    assert_eq!(sell.triggers[0].trigger_type, TriggerType::Action);
    assert_eq!(sell.triggers[0].source_id.as_deref(), Some("buy-eth-spot"));
}

#[test]
fn data_requirements_evm_swap() {
    let c = compile(&graph(&sample_graphs()["g01_evm_swap_buy_eth_base"])).unwrap();
    let candles: Vec<_> = c.data_requirements.candles.iter().map(|r| (r.venue.as_str(), r.symbol.as_str())).collect();
    assert_eq!(candles, vec![("base", "ETH")]);
    assert_eq!(c.data_requirements.gas.iter().map(|g| g.chain.as_str()).collect::<Vec<_>>(), vec!["base"]);
    assert!(c.data_requirements.funding.is_empty());
}

#[test]
fn data_requirements_perp_has_funding_no_gas() {
    let c = compile(&graph(&sample_graphs()["g05_hl_perp_open_long"])).unwrap();
    assert_eq!(c.data_requirements.funding.len(), 1);
    assert!(c.data_requirements.gas.is_empty()); // hyperliquid carries no EVM gas
}

#[test]
fn data_requirements_yield_no_candles() {
    let c = compile(&graph(&sample_graphs()["g08_evm_yield_deposit"])).unwrap();
    assert_eq!(c.data_requirements.yields.len(), 1);
    assert!(c.data_requirements.candles.is_empty()); // USDC is stable
    assert_eq!(c.data_requirements.gas.len(), 1);
}

#[test]
fn signal_price_feed_uses_traded_venue_when_unambiguous() {
    // g12 trades ETH only on base, so ETH signals resolve to base candles.
    let c = compile(&graph(&sample_graphs()["g12_evm_swap_dca_ladder"])).unwrap();
    let venues: Vec<_> = c.data_requirements.candles.iter().map(|r| r.venue.as_str()).collect();
    assert!(venues.iter().all(|v| *v == "base"));
}

#[test]
fn errors_are_clear() {
    // duplicate id
    let dup = serde_json::json!({"nodes":[
        {"id":"d","kind":"action","subtype":"swap","config":{"from_asset":"USDC","to_asset":"ETH","amount":"1","chain":"base"}},
        {"id":"d","kind":"action","subtype":"swap","config":{"from_asset":"USDC","to_asset":"ETH","amount":"1","chain":"base"}}
    ],"edges":[]});
    assert!(compile(&graph(&dup)).unwrap_err().message.contains("duplicate"));

    // edge to unknown node
    let mut g = sample_graphs()["g01_evm_swap_buy_eth_base"].clone();
    g["edges"] = serde_json::json!([{"from":"buy-eth-on-base","to":"ghost"}]);
    assert!(compile(&graph(&g)).unwrap_err().message.contains("unknown target"));

    // malformed config carries node id
    let bad = serde_json::json!({"nodes":[
        {"id":"bad","kind":"action","subtype":"swap","config":{"from_asset":"USDC","to_asset":"ETH","chain":"base"}}
    ],"edges":[]});
    assert_eq!(compile(&graph(&bad)).unwrap_err().node_id.as_deref(), Some("bad"));

    // empty graph
    assert!(compile(&graph(&serde_json::json!({"nodes":[],"edges":[]}))).unwrap_err().message.contains("no nodes"));
}

#[test]
fn disabled_node_excluded_with_warning() {
    let mut g = sample_graphs()["g03_hl_spot_buy_then_sell"].clone();
    g["nodes"][1]["enabled"] = Value::Bool(false);
    let c = compile(&graph(&g)).unwrap();
    assert_eq!(c.actions.len(), 1);
    assert!(c.warnings.iter().any(|w| w.contains("disabled")));
}
