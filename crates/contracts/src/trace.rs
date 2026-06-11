//! Simulation trace contract (simulation-trace.schema.json).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::policy::SimulationPolicy;
use crate::Decimal;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerpPosition {
    pub venue: String,
    pub symbol: String,
    pub side: String,
    pub size: Decimal,
    pub entry_price: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leverage: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin_usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquidation_price: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YieldPosition {
    pub protocol: String,
    pub asset: String,
    pub chain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    pub principal: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accrued: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Portfolio {
    /// venue -> asset -> decimal-string balance
    #[serde(default)]
    pub balances: BTreeMap<String, BTreeMap<String, Decimal>>,
    #[serde(default)]
    pub perp_positions: Vec<PerpPosition>,
    #[serde(default)]
    pub yield_positions: Vec<YieldPosition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub ts: String,
    pub equity_usd: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portfolio: Option<Portfolio>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub ts: String,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulationTrace {
    #[serde(default = "default_trace_schema_version")]
    pub schema_version: String,
    pub policy: SimulationPolicy,
    pub interval: String,
    pub start: String,
    pub end: String,
    /// First actual tick of the run (#167): the tick clock is data-driven, so
    /// this may be later than the requested `start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_start: Option<String>,
    /// Last actual tick of the run (#167); may be earlier than `end`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_end: Option<String>,
    #[serde(default)]
    pub snapshots: Vec<Snapshot>,
    #[serde(default)]
    pub events: Vec<Event>,
    pub final_portfolio: Portfolio,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub errors: Vec<String>,
}

fn default_trace_schema_version() -> String {
    "catalyst.backtest.trace.v1".to_string()
}
