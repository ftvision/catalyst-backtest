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
    /// Generalized signal: compare any [`Source`] against a [`Reference`].
    /// `price_threshold` is the price-only sugar for this.
    Threshold,
    /// Boolean combinator signals. Their inputs are the upstream signals with an
    /// edge into them (signal -> signal edges, allowed only for these subtypes).
    All,
    Any,
    Not,
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

/// What a relative [`Amount`] is a percentage of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmountBasis {
    /// % of the relevant asset balance (swap from-asset, yield asset, perp cash).
    PctBalance,
    /// % of the relevant open position (perp notional, yield principal+accrued).
    PctPosition,
    /// % of total portfolio equity in USD.
    PctPortfolio,
}

/// An action amount: either an absolute quantity (a decimal string, or the
/// `"all"` sentinel) or a percentage of a [`AmountBasis`]. Bare strings still
/// deserialize as [`Amount::Absolute`], so existing graphs are unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Amount {
    Absolute(Decimal),
    Relative { basis: AmountBasis, value: Decimal },
}

impl Amount {
    /// The `"all"` full-balance sentinel.
    pub fn is_all(&self) -> bool {
        matches!(self, Amount::Absolute(s) if s == "all")
    }
    /// The absolute decimal string. Relative amounts are resolved to absolute
    /// before execution, so this returns `"0"` for an unresolved relative amount.
    pub fn as_str(&self) -> &str {
        match self {
            Amount::Absolute(s) => s,
            Amount::Relative { .. } => "0",
        }
    }
}

impl From<&str> for Amount {
    fn from(s: &str) -> Self {
        Amount::Absolute(s.to_string())
    }
}
impl From<String> for Amount {
    fn from(s: String) -> Self {
        Amount::Absolute(s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwapConfig {
    pub from_asset: String,
    pub to_asset: String,
    pub amount: Amount,
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
    pub size_usd: Amount,
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
    pub amount: Amount,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceThresholdConfig {
    pub symbol: String,
    pub operator: String,
    pub threshold: Decimal,
}

/// The scalar a signal observes each tick. Each variant maps 1:1 onto a
/// market-data kind, so a `Source` drives both data-requirement extraction
/// (in the compiler) and the per-tick value read (in the engine). Adding a new
/// data kind is one arm here, not a new node subtype.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// Spot/mark price of a symbol. `venue` pins the candle series; when omitted
    /// the compiler resolves it from the graph (unambiguous venue) or a default.
    Price {
        symbol: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        venue: Option<String>,
    },
    /// Perp funding rate at a venue.
    Funding { venue: String, symbol: String },
    /// Protocol yield (APR) for an asset/chain/pool.
    Yield {
        protocol: String,
        asset: String,
        chain: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pool: Option<String>,
    },
    /// Per-chain gas cost in USD.
    Gas { chain: String },
}

/// The right-hand side of a signal comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Reference {
    /// A constant threshold, e.g. `{ "const": "1800" }`.
    Const {
        #[serde(rename = "const")]
        value: Decimal,
    },
    /// Another source, e.g. price vs a moving average or funding vs zero:
    /// `{ "source": { "kind": "funding", ... } }`.
    Source { source: Source },
    /// A graph variable, e.g. `{ "var": "entry_price" }`. Resolved from
    /// `Graph.variables` (see issue #49 for full variable substitution).
    Var { var: String },
}

/// Generalized threshold signal config (`subtype: "threshold"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThresholdConfig {
    pub source: Source,
    pub operator: String,
    pub reference: Reference,
}
