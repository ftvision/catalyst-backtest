//! Catalyst graph contract (graph.schema.json).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Decimal;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Action,
    Signal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeSubtype {
    Swap,
    PerpOrder,
    YieldDeposit,
    YieldWithdraw,
    PriceThreshold,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub subtype: NodeSubtype,
    /// Free-form so the contract round-trips any producer payload; the typed
    /// config structs below are used by the graph compiler to normalize it.
    pub config: Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    // `from` is a valid Rust identifier (not a keyword), so it serializes as
    // "from" with no rename needed — unlike `Event.type`/`SimulationPolicy.yield`.
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default = "default_graph_schema_version")]
    pub schema_version: String,
    #[serde(default)]
    pub variables: Value,
    #[serde(default)]
    pub settings: Value,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

fn default_graph_schema_version() -> String {
    "catalyst.graph.definition.v1".to_string()
}

// --- Typed config structs (mirror the per-subtype config schemas) ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwapConfig {
    pub from_asset: String,
    pub to_asset: String,
    pub amount: Decimal,
    pub chain: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerpSide {
    Long,
    Short,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerpOrderConfig {
    pub symbol: String,
    pub side: PerpSide,
    pub size_usd: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leverage: Option<Decimal>,
    pub chain: String,
    #[serde(default = "default_order_type")]
    pub order_type: String,
    #[serde(default)]
    pub reduce_only: bool,
}

fn default_order_type() -> String {
    "market".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YieldConfig {
    pub chain: String,
    pub protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    pub asset: String,
    pub amount: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceThresholdConfig {
    pub symbol: String,
    pub operator: String,
    pub threshold: Decimal,
}
