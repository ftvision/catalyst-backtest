//! Derive the executable structure from a raw graph: which actions are initial,
//! which signals drive which actions, and which actions chain to others.
//!
//! This mirrors the Python graph compiler's trigger semantics so the Rust engine
//! can run a validated graph directly.

use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;

use catalyst_contracts::graph::{Graph, NodeKind, NodeSubtype};

fn subtype_str(subtype: &NodeSubtype) -> String {
    match subtype {
        NodeSubtype::Swap => "swap",
        NodeSubtype::PerpOrder => "perp_order",
        NodeSubtype::YieldDeposit => "yield_deposit",
        NodeSubtype::YieldWithdraw => "yield_withdraw",
        NodeSubtype::PriceThreshold => "price_threshold",
    }
    .to_string()
}

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
    pub fn from_graph(graph: &Graph) -> Self {
        let enabled: HashSet<&str> =
            graph.nodes.iter().filter(|n| n.enabled).map(|n| n.id.as_str()).collect();
        let kind: HashMap<&str, &NodeKind> =
            graph.nodes.iter().map(|n| (n.id.as_str(), &n.kind)).collect();

        // Edges restricted to enabled endpoints.
        let edges: Vec<(&str, &str)> = graph
            .edges
            .iter()
            .filter(|e| enabled.contains(e.from.as_str()) && enabled.contains(e.to.as_str()))
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();

        let mut g = ExecGraph::default();

        for node in graph.nodes.iter().filter(|n| n.enabled) {
            match node.kind {
                NodeKind::Action => {
                    g.actions.insert(
                        node.id.clone(),
                        ActionNode {
                            id: node.id.clone(),
                            subtype: subtype_str(&node.subtype),
                            config: node.config.clone(),
                        },
                    );
                    let has_incoming = edges.iter().any(|(_, to)| *to == node.id);
                    if !has_incoming {
                        g.initial_actions.push(node.id.clone());
                    }
                }
                NodeKind::Signal => {
                    let cfg = &node.config;
                    let symbol = cfg.get("symbol").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let operator =
                        cfg.get("operator").and_then(|v| v.as_str()).unwrap_or("<").to_string();
                    let threshold = cfg
                        .get("threshold")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(Decimal::ZERO);
                    let targets = edges
                        .iter()
                        .filter(|(from, _)| *from == node.id)
                        .map(|(_, to)| to.to_string())
                        .collect();
                    g.signals.push(Signal { id: node.id.clone(), symbol, operator, threshold, targets });
                }
            }
        }

        // Action -> action chaining edges.
        for (from, to) in &edges {
            let from_is_action = matches!(kind.get(from), Some(NodeKind::Action));
            let to_is_action = matches!(kind.get(to), Some(NodeKind::Action));
            if from_is_action && to_is_action {
                g.out_action_edges.entry(from.to_string()).or_default().push(to.to_string());
            }
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
