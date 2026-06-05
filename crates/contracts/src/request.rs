//! Backtest request contract (backtest-request.schema.json).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::Decimal;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PolicySelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ExecutionOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slippage_bps: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_cooldown: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub start: String,
    pub end: String,
    pub interval: String,
    /// venue -> asset -> decimal-string amount
    pub initial_portfolio: BTreeMap<String, BTreeMap<String, Decimal>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionOverrides>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestRequest {
    #[serde(default = "default_request_schema_version")]
    pub schema_version: String,
    pub graph: Graph,
    #[serde(default)]
    pub policy: PolicySelector,
    pub config: BacktestConfig,
}

fn default_request_schema_version() -> String {
    "catalyst.backtest.request.v1".to_string()
}
