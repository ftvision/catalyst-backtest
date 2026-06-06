//! Compile a raw Catalyst graph into a normalized [`CompiledGraph`].
//!
//! Rust port of the Python `catalyst_graph_compiler` (per ADR 0001 the run path
//! is Rust). It validates the graph, classifies execution triggers
//! (initial / signal-driven / action-chained), and extracts the data
//! requirements the market-data layer must source. This is the single
//! authoritative trigger derivation; the simulation engine consumes it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use catalyst_contracts::graph::{
    Graph, Node, NodeKind, NodeSubtype, PerpOrderConfig, PriceThresholdConfig, SwapConfig,
    YieldConfig,
};

/// Assets treated as cash/quote (no price feed needed to value them in USD).
const STABLE_ASSETS: &[&str] = &["USDC", "USDT", "USD", "DAI", "USDC.E"];
/// Fallback price feed for signals/symbols not traded on a single explicit venue.
pub const DEFAULT_PRICE_VENUE: &str = "hyperliquid";

fn is_stable(asset: &str) -> bool {
    let up = asset.to_ascii_uppercase();
    STABLE_ASSETS.contains(&up.as_str())
}

fn subtype_str(s: &NodeSubtype) -> String {
    match s {
        NodeSubtype::Swap => "swap",
        NodeSubtype::PerpOrder => "perp_order",
        NodeSubtype::YieldDeposit => "yield_deposit",
        NodeSubtype::YieldWithdraw => "yield_withdraw",
        NodeSubtype::PriceThreshold => "price_threshold",
    }
    .to_string()
}

// --- errors ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub message: String,
    pub node_id: Option<String>,
}

impl CompileError {
    fn new(message: impl Into<String>, node_id: Option<String>) -> Self {
        CompileError { message: message.into(), node_id }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.node_id {
            Some(id) => write!(f, "{} (node {id:?})", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}
impl std::error::Error for CompileError {}

// --- compiled output ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    Initial,
    Signal,
    Action,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    #[serde(rename = "type")]
    pub trigger_type: TriggerType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledAction {
    pub id: String,
    pub subtype: String,
    pub config: Value,
    pub triggers: Vec<Trigger>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledSignal {
    pub id: String,
    pub subtype: String,
    pub config: Value,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CandleReq {
    pub venue: String,
    pub symbol: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FundingReq {
    pub venue: String,
    pub symbol: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GasReq {
    pub chain: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct YieldReq {
    pub protocol: String,
    pub asset: String,
    pub chain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DataRequirements {
    pub candles: Vec<CandleReq>,
    pub funding: Vec<FundingReq>,
    pub gas: Vec<GasReq>,
    pub yields: Vec<YieldReq>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledGraph {
    pub schema_version: String,
    pub actions: Vec<CompiledAction>,
    pub signals: Vec<CompiledSignal>,
    pub data_requirements: DataRequirements,
    pub warnings: Vec<String>,
}

// --- typed configs of enabled nodes (validated) ---

enum Typed {
    Swap(SwapConfig),
    Perp(PerpOrderConfig),
    Yield(YieldConfig),
    Threshold(PriceThresholdConfig),
}

fn validate_node(node: &Node) -> Result<Typed, CompileError> {
    let id = || Some(node.id.clone());
    let bad = |e: serde_json::Error, what: &str| {
        CompileError::new(format!("invalid {what} config: {e}"), Some(node.id.clone()))
    };
    match (&node.kind, &node.subtype) {
        (NodeKind::Action, NodeSubtype::Swap) => serde_json::from_value(node.config.clone())
            .map(Typed::Swap)
            .map_err(|e| bad(e, "swap")),
        (NodeKind::Action, NodeSubtype::PerpOrder) => serde_json::from_value(node.config.clone())
            .map(Typed::Perp)
            .map_err(|e| bad(e, "perp_order")),
        (NodeKind::Action, NodeSubtype::YieldDeposit | NodeSubtype::YieldWithdraw) => {
            serde_json::from_value(node.config.clone()).map(Typed::Yield).map_err(|e| bad(e, "yield"))
        }
        (NodeKind::Signal, NodeSubtype::PriceThreshold) => {
            serde_json::from_value(node.config.clone())
                .map(Typed::Threshold)
                .map_err(|e| bad(e, "price_threshold"))
        }
        (NodeKind::Action, other) => {
            Err(CompileError::new(format!("unsupported action subtype {}", subtype_str(other)), id()))
        }
        (NodeKind::Signal, other) => {
            Err(CompileError::new(format!("unsupported signal subtype {}", subtype_str(other)), id()))
        }
    }
}

/// Compile and validate a graph into a [`CompiledGraph`].
pub fn compile(graph: &Graph) -> Result<CompiledGraph, CompileError> {
    if graph.nodes.is_empty() {
        return Err(CompileError::new("graph has no nodes", None));
    }

    let mut warnings: Vec<String> = Vec::new();

    // Duplicate ids.
    let mut seen: HashSet<&str> = HashSet::new();
    for node in &graph.nodes {
        if !seen.insert(node.id.as_str()) {
            return Err(CompileError::new("duplicate node id", Some(node.id.clone())));
        }
    }

    let all_ids: HashSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut kind_of: HashMap<&str, &NodeKind> = HashMap::new();
    let mut enabled: HashSet<&str> = HashSet::new();
    for node in &graph.nodes {
        if node.enabled {
            enabled.insert(node.id.as_str());
            kind_of.insert(node.id.as_str(), &node.kind);
        } else {
            warnings.push(format!("node {:?} is disabled and was excluded", node.id));
        }
    }

    // Validate enabled node kind/subtype + config.
    let mut typed: HashMap<&str, Typed> = HashMap::new();
    for node in graph.nodes.iter().filter(|n| n.enabled) {
        typed.insert(node.id.as_str(), validate_node(node)?);
    }

    // Validate + filter edges.
    let mut edges: Vec<(&str, &str)> = Vec::new();
    for edge in &graph.edges {
        if !all_ids.contains(edge.from.as_str()) {
            return Err(CompileError::new(
                format!("edge references unknown source node {:?}", edge.from),
                None,
            ));
        }
        if !all_ids.contains(edge.to.as_str()) {
            return Err(CompileError::new(
                format!("edge references unknown target node {:?}", edge.to),
                None,
            ));
        }
        if !enabled.contains(edge.from.as_str()) || !enabled.contains(edge.to.as_str()) {
            warnings.push(format!(
                "edge {:?} -> {:?} touches a disabled node and was dropped",
                edge.from, edge.to
            ));
            continue;
        }
        if matches!(kind_of.get(edge.to.as_str()), Some(NodeKind::Signal)) {
            warnings.push(format!(
                "edge {:?} -> {:?} targets a signal; signals are evaluated by their own threshold, \
                 so this edge has no effect",
                edge.from, edge.to
            ));
            continue;
        }
        edges.push((edge.from.as_str(), edge.to.as_str()));
    }

    let symbol_venues = symbol_venue_map(&typed);

    // Actions with triggers (graph order).
    let mut actions = Vec::new();
    for node in &graph.nodes {
        if !enabled.contains(node.id.as_str()) || !matches!(node.kind, NodeKind::Action) {
            continue;
        }
        let incoming: Vec<&str> =
            edges.iter().filter(|(_, to)| *to == node.id).map(|(from, _)| *from).collect();
        let triggers = if incoming.is_empty() {
            vec![Trigger { trigger_type: TriggerType::Initial, source_id: None }]
        } else {
            incoming
                .iter()
                .map(|src| {
                    let t = if matches!(kind_of.get(src), Some(NodeKind::Signal)) {
                        TriggerType::Signal
                    } else {
                        TriggerType::Action
                    };
                    Trigger { trigger_type: t, source_id: Some(src.to_string()) }
                })
                .collect()
        };
        actions.push(CompiledAction {
            id: node.id.clone(),
            subtype: subtype_str(&node.subtype),
            config: node.config.clone(),
            triggers,
        });
    }

    // Signals with targets.
    let mut signals = Vec::new();
    for node in &graph.nodes {
        if !enabled.contains(node.id.as_str()) || !matches!(node.kind, NodeKind::Signal) {
            continue;
        }
        let targets: Vec<String> =
            edges.iter().filter(|(from, _)| *from == node.id).map(|(_, to)| to.to_string()).collect();
        if targets.is_empty() {
            warnings.push(format!("signal {:?} has no downstream actions", node.id));
        }
        signals.push(CompiledSignal {
            id: node.id.clone(),
            subtype: subtype_str(&node.subtype),
            config: node.config.clone(),
            targets,
        });
    }

    let data_requirements = data_requirements(&graph.nodes, &enabled, &typed, &symbol_venues);

    Ok(CompiledGraph {
        schema_version: "catalyst.backtest.compiled_graph.v1".to_string(),
        actions,
        signals,
        data_requirements,
        warnings,
    })
}

fn symbol_venue_map(typed: &HashMap<&str, Typed>) -> HashMap<String, String> {
    let mut venues: HashMap<String, BTreeSet<String>> = HashMap::new();
    for cfg in typed.values() {
        match cfg {
            Typed::Swap(s) => {
                for asset in [&s.from_asset, &s.to_asset] {
                    if !is_stable(asset) {
                        venues.entry(asset.clone()).or_default().insert(s.chain.clone());
                    }
                }
            }
            Typed::Perp(p) => {
                venues.entry(p.symbol.clone()).or_default().insert(p.chain.clone());
            }
            _ => {}
        }
    }
    venues
        .into_iter()
        .filter(|(_, vs)| vs.len() == 1)
        .map(|(sym, vs)| (sym, vs.into_iter().next().unwrap()))
        .collect()
}

fn data_requirements(
    nodes: &[Node],
    enabled: &HashSet<&str>,
    typed: &HashMap<&str, Typed>,
    symbol_venues: &HashMap<String, String>,
) -> DataRequirements {
    let mut candles: BTreeMap<(String, String), CandleReq> = BTreeMap::new();
    let mut funding: BTreeMap<(String, String), FundingReq> = BTreeMap::new();
    let mut gas: BTreeMap<String, GasReq> = BTreeMap::new();
    let mut yields: BTreeMap<(String, String, String, Option<String>), YieldReq> = BTreeMap::new();

    let mut add_candle = |venue: &str, symbol: &str| {
        candles.insert(
            (venue.to_string(), symbol.to_string()),
            CandleReq { venue: venue.to_string(), symbol: symbol.to_string() },
        );
    };

    // Iterate in graph order for determinism, only enabled nodes.
    for node in nodes.iter().filter(|n| enabled.contains(n.id.as_str())) {
        match typed.get(node.id.as_str()) {
            Some(Typed::Swap(s)) => {
                for asset in [&s.from_asset, &s.to_asset] {
                    if !is_stable(asset) {
                        add_candle(&s.chain, asset);
                    }
                }
                if s.chain != "hyperliquid" {
                    gas.insert(s.chain.clone(), GasReq { chain: s.chain.clone() });
                }
            }
            Some(Typed::Perp(p)) => {
                add_candle(&p.chain, &p.symbol);
                funding.insert(
                    (p.chain.clone(), p.symbol.clone()),
                    FundingReq { venue: p.chain.clone(), symbol: p.symbol.clone() },
                );
            }
            Some(Typed::Yield(y)) => {
                yields.insert(
                    (y.protocol.clone(), y.asset.clone(), y.chain.clone(), y.pool.clone()),
                    YieldReq {
                        protocol: y.protocol.clone(),
                        asset: y.asset.clone(),
                        chain: y.chain.clone(),
                        pool: y.pool.clone(),
                    },
                );
                if y.chain != "hyperliquid" {
                    gas.insert(y.chain.clone(), GasReq { chain: y.chain.clone() });
                }
            }
            Some(Typed::Threshold(t)) => {
                let venue =
                    symbol_venues.get(&t.symbol).cloned().unwrap_or_else(|| DEFAULT_PRICE_VENUE.into());
                add_candle(&venue, &t.symbol);
            }
            None => {}
        }
    }

    DataRequirements {
        candles: candles.into_values().collect(),
        funding: funding.into_values().collect(),
        gas: gas.into_values().collect(),
        yields: yields.into_values().collect(),
    }
}
