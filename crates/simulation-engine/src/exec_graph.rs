//! The engine's internal executable view, built from a [`CompiledGraph`].
//!
//! Trigger derivation lives in `catalyst-graph-compiler` (the single authoritative
//! implementation, per ADR 0001). This module just reorganizes the compiler's
//! output into the lookups the tick loop needs.

use std::collections::HashMap;

use rust_decimal::Decimal;

use catalyst_graph_compiler::{CompiledGraph, TriggerType};

#[derive(Debug, Clone)]
pub struct Signal {
    pub id: String,
    pub symbol: String,
    pub operator: String,
    pub threshold: Decimal,
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
            let cfg = &signal.config;
            let symbol = cfg.get("symbol").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let operator = cfg.get("operator").and_then(|v| v.as_str()).unwrap_or("<").to_string();
            let threshold = cfg
                .get("threshold")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(Decimal::ZERO);
            g.signals.push(Signal {
                id: signal.id.clone(),
                symbol,
                operator,
                threshold,
                targets: signal.targets.clone(),
            });
        }

        g
    }
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
