//! The engine's internal executable view, built from a [`CompiledGraph`].
//!
//! Trigger derivation lives in `catalyst-graph-compiler` (the single authoritative
//! implementation, per ADR 0001). This module just reorganizes the compiler's
//! output into the lookups the tick loop needs.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde_json::Value;

use catalyst_contracts::graph::{PriceThresholdConfig, Reference, Source, ThresholdConfig};
use catalyst_graph_compiler::{CompiledGraph, TriggerType};

#[derive(Debug, Clone)]
pub enum CombinatorOp {
    All,
    Any,
    Not,
}

#[derive(Debug, Clone)]
pub enum SignalDef {
    /// Leaf: compare a market-data source against a reference.
    Threshold { source: Source, operator: String, reference: Reference },
    /// Boolean combinator over upstream signals (by id).
    Combinator { op: CombinatorOp, inputs: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub id: String,
    pub def: SignalDef,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ActionNode {
    pub id: String,
    pub subtype: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Default)]
pub struct ExecGraph {
    pub initial_actions: Vec<String>,
    pub signals: Vec<Signal>,
    pub actions: HashMap<String, ActionNode>,
    /// action id -> downstream action ids it chains into.
    pub out_action_edges: HashMap<String, Vec<String>>,
}

impl ExecGraph {
    /// Build the engine's execution view from a compiled graph.
    pub fn from_compiled(compiled: &CompiledGraph) -> Self {
        let mut g = ExecGraph::default();

        for action in &compiled.actions {
            g.actions.insert(
                action.id.clone(),
                ActionNode {
                    id: action.id.clone(),
                    subtype: action.subtype.clone(),
                    config: action.config.clone(),
                },
            );
            for trigger in &action.triggers {
                match trigger.trigger_type {
                    TriggerType::Initial => g.initial_actions.push(action.id.clone()),
                    // action chains: source action -> this action
                    TriggerType::Action => {
                        if let Some(src) = &trigger.source_id {
                            g.out_action_edges.entry(src.clone()).or_default().push(action.id.clone());
                        }
                    }
                    // signal triggers are captured via the signal's `targets`.
                    TriggerType::Signal => {}
                }
            }
        }

        for signal in &compiled.signals {
            let def = match signal.subtype.as_str() {
                "all" => SignalDef::Combinator {
                    op: CombinatorOp::All,
                    inputs: signal.inputs.clone(),
                },
                "any" => SignalDef::Combinator {
                    op: CombinatorOp::Any,
                    inputs: signal.inputs.clone(),
                },
                "not" => SignalDef::Combinator {
                    op: CombinatorOp::Not,
                    inputs: signal.inputs.clone(),
                },
                _ => {
                    let (source, operator, reference) = parse_signal_config(&signal.config);
                    SignalDef::Threshold { source, operator, reference }
                }
            };
            g.signals.push(Signal { id: signal.id.clone(), def, targets: signal.targets.clone() });
        }

        g
    }
}

/// Normalize a compiled signal's raw config into (source, operator, reference).
/// Accepts both the generalized `threshold` shape and the `price_threshold`
/// sugar (the compiler leaves the original config on `CompiledSignal`).
fn parse_signal_config(cfg: &Value) -> (Source, String, Reference) {
    if let Ok(t) = serde_json::from_value::<ThresholdConfig>(cfg.clone()) {
        return (t.source, t.operator, t.reference);
    }
    if let Ok(p) = serde_json::from_value::<PriceThresholdConfig>(cfg.clone()) {
        return (
            Source::Price { symbol: p.symbol, venue: None },
            p.operator,
            Reference::Const { value: p.threshold },
        );
    }
    (
        Source::Price { symbol: String::new(), venue: None },
        "<".to_string(),
        Reference::Const { value: "0".to_string() },
    )
}

/// Evaluate a price-threshold condition.
pub fn eval_threshold(price: Decimal, operator: &str, threshold: Decimal) -> bool {
    match operator {
        "<" => price < threshold,
        "<=" => price <= threshold,
        ">" => price > threshold,
        ">=" => price >= threshold,
        "==" => price == threshold,
        "!=" => price != threshold,
        _ => false,
    }
}
