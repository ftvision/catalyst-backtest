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
    /// `market` (fill at the current bar) or `limit` (rest until touched).
    #[serde(default = "default_order_type")]
    pub order_type: String,
    /// Required when `order_type` is `limit`: the worst acceptable price (in
    /// quote/USD per base unit). A buy fills at/below it; a sell fills at/above it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<Decimal>,
    /// `gtc` (default) or `good_til_bars`. Only meaningful for limit orders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<String>,
    /// With `good_til_bars`, the order expires this many bars after placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expire_after_bars: Option<u32>,
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
    /// Required when `order_type` is `limit`: the worst acceptable mark price.
    /// Opening a long / closing a short fills at/below it; opening a short /
    /// closing a long fills at/above it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<Decimal>,
    /// `gtc` (default) or `good_til_bars`. Only meaningful for limit orders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<String>,
    /// With `good_til_bars`, the order expires this many bars after placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expire_after_bars: Option<u32>,
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
