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

// --- ADR 0002: generalized threshold sources drive data requirements ---

#[test]
fn funding_threshold_source_requires_funding() {
    let g = graph(&serde_json::json!({
        "nodes": [
            {"id": "rich", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "funding", "venue": "hyperliquid", "symbol": "ETH"},
                        "operator": ">=", "reference": {"const": "0.0001"}}},
            {"id": "short", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "short", "size_usd": "500", "chain": "hyperliquid"}}
        ],
        "edges": [{"from": "rich", "to": "short"}]
    }));
    let c = compile(&g).unwrap();
    assert!(c
        .data_requirements
        .funding
        .iter()
        .any(|f| f.venue == "hyperliquid" && f.symbol == "ETH"));
    assert_eq!(c.signals.len(), 1);
    assert_eq!(c.signals[0].subtype, "threshold");
}

#[test]
fn yield_threshold_source_requires_yields() {
    let g = graph(&serde_json::json!({
        "nodes": [
            {"id": "apr", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "yield", "protocol": "aave", "asset": "USDC",
                                   "chain": "base", "pool": "usdc"},
                        "operator": ">=", "reference": {"const": "0.05"}}},
            {"id": "dep", "kind": "action", "subtype": "yield_deposit",
             "config": {"chain": "base", "protocol": "aave", "pool": "usdc",
                        "asset": "USDC", "amount": "100"}}
        ],
        "edges": [{"from": "apr", "to": "dep"}]
    }));
    let c = compile(&g).unwrap();
    assert!(c.data_requirements.yields.iter().any(|y| y.protocol == "aave"
        && y.asset == "USDC"
        && y.chain == "base"
        && y.pool.as_deref() == Some("usdc")));
}

#[test]
fn price_threshold_sugar_still_requires_candles() {
    let g = graph(&serde_json::json!({
        "nodes": [
            {"id": "below", "kind": "signal", "subtype": "price_threshold",
             "config": {"symbol": "ETH", "operator": "<", "threshold": "1800"}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "hyperliquid"}}
        ],
        "edges": [{"from": "below", "to": "buy"}]
    }));
    let c = compile(&g).unwrap();
    assert!(c.data_requirements.candles.iter().any(|cd| cd.symbol == "ETH"));
    // sugar normalizes to the same subtype label it was authored with
    assert_eq!(c.signals[0].subtype, "price_threshold");
}

// --- ADR 0002 step 4: combinator signals ---

fn leaf_node(id: &str, op: &str, threshold: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "kind": "signal", "subtype": "threshold",
        "config": {"source": {"kind": "price", "symbol": "ETH"},
                   "operator": op, "reference": {"const": threshold}}})
}

#[test]
fn combinator_records_inputs_in_topological_order() {
    let g = graph(&serde_json::json!({
        "nodes": [
            {"id": "band", "kind": "signal", "subtype": "all", "config": {}},
            leaf_node("hi", "<", "2000"),
            leaf_node("lo", ">", "1000"),
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}}
        ],
        "edges": [
            {"from": "hi", "to": "band"},
            {"from": "lo", "to": "band"},
            {"from": "band", "to": "buy"}
        ]
    }));
    let c = compile(&g).unwrap();
    let band = c.signals.iter().find(|s| s.id == "band").unwrap();
    assert_eq!(band.subtype, "all");
    let mut inputs = band.inputs.clone();
    inputs.sort();
    assert_eq!(inputs, vec!["hi".to_string(), "lo".to_string()]);
    assert!(band.targets.contains(&"buy".to_string()));
    // leaves do not carry the combinator as an action target
    let hi = c.signals.iter().find(|s| s.id == "hi").unwrap();
    assert!(hi.targets.is_empty());
    // topological order: inputs precede the combinator that reads them
    let pos = |id: &str| c.signals.iter().position(|s| s.id == id).unwrap();
    assert!(pos("hi") < pos("band"));
    assert!(pos("lo") < pos("band"));
}

#[test]
fn not_with_two_inputs_is_rejected() {
    let g = graph(&serde_json::json!({
        "nodes": [
            leaf_node("a", "<", "2000"),
            leaf_node("b", ">", "1000"),
            {"id": "n", "kind": "signal", "subtype": "not", "config": {}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}}
        ],
        "edges": [
            {"from": "a", "to": "n"},
            {"from": "b", "to": "n"},
            {"from": "n", "to": "buy"}
        ]
    }));
    assert!(compile(&g).is_err());
}

#[test]
fn combinator_cycle_is_rejected() {
    let g = graph(&serde_json::json!({
        "nodes": [
            {"id": "x", "kind": "signal", "subtype": "all", "config": {}},
            {"id": "y", "kind": "signal", "subtype": "all", "config": {}}
        ],
        "edges": [
            {"from": "x", "to": "y"},
            {"from": "y", "to": "x"}
        ]
    }));
    let err = compile(&g).unwrap_err();
    assert!(err.to_string().contains("cycle"));
}

// --- ADR 0002 step 3: derived sources + warmup ---

#[test]
fn derived_reference_sets_lookback_and_requires_candles() {
    let g = graph(&serde_json::json!({
        "nodes": [
            {"id": "below-ma", "kind": "signal", "subtype": "threshold",
             "config": {
                 "source": {"kind": "price", "symbol": "ETH", "venue": "base"},
                 "operator": "<",
                 "reference": {"source": {"kind": "derived",
                     "of": {"kind": "price", "symbol": "ETH", "venue": "base"},
                     "transform": "sma", "window": 20}}
             }},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}}
        ],
        "edges": [{"from": "below-ma", "to": "buy"}]
    }));
    let c = compile(&g).unwrap();
    assert_eq!(c.data_requirements.lookback_bars, 20);
    assert!(c.data_requirements.candles.iter().any(|cd| cd.symbol == "ETH" && cd.venue == "base"));
}

// --- #49: graph variables / settings ---

fn var_swap_graph(amount: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "variables": {"size": "250"},
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": amount, "chain": "base"}
        }],
        "edges": []
    })
}

#[test]
fn variable_substitutes_into_action_amount() {
    let c = compile(&graph(&var_swap_graph(serde_json::json!("$size")))).unwrap();
    assert_eq!(c.actions[0].config["amount"], serde_json::json!("250"));
    assert_eq!(c.resolved_variables.get("size"), Some(&serde_json::json!("250")));
}

#[test]
fn undefined_variable_is_a_compile_error() {
    let g = serde_json::json!({
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "$missing", "chain": "base"}
        }],
        "edges": []
    });
    let err = compile(&graph(&g)).unwrap_err();
    assert!(err.to_string().contains("undefined variable"), "{err}");
    assert!(err.to_string().contains("missing"), "{err}");
}

#[test]
fn unused_variable_warns() {
    let g = serde_json::json!({
        "variables": {"size": "250", "ghost": "1"},
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "$size", "chain": "base"}
        }],
        "edges": []
    });
    let c = compile(&graph(&g)).unwrap();
    assert!(c.warnings.iter().any(|w| w.contains("ghost") && w.contains("never used")), "{:?}", c.warnings);
    assert!(!c.resolved_variables.contains_key("ghost"));
}

#[test]
fn var_reference_object_resolves_to_const() {
    let g = serde_json::json!({
        "variables": {"floor": "1800"},
        "nodes": [
            {"id": "below", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "price", "symbol": "ETH"},
                        "operator": "<", "reference": {"var": "floor"}}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}}
        ],
        "edges": [{"from": "below", "to": "buy"}]
    });
    let c = compile(&graph(&g)).unwrap();
    let sig = c.signals.iter().find(|s| s.id == "below").unwrap();
    assert_eq!(sig.config["reference"], serde_json::json!({"const": "1800"}));
}

#[test]
fn settings_with_keys_warn_and_non_object_variables_error() {
    let g = serde_json::json!({
        "settings": {"foo": "bar"},
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}
        }],
        "edges": []
    });
    let c = compile(&graph(&g)).unwrap();
    assert!(c.warnings.iter().any(|w| w.contains("settings") && w.contains("not yet honored")), "{:?}", c.warnings);

    let bad = serde_json::json!({
        "variables": [1, 2],
        "nodes": [{"id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "10", "chain": "base"}}],
        "edges": []
    });
    assert!(compile(&graph(&bad)).is_err());
}
