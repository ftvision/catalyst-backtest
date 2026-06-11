//! Backtest result contract (backtest-result.schema.json).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::policy::SimulationPolicy;
use crate::trace::Portfolio;
use crate::Decimal;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub starting_value_usd: Decimal,
    pub final_value_usd: Decimal,
    pub pnl_usd: Decimal,
    pub return_pct: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_drawdown_pct: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquityPoint {
    pub ts: String,
    pub equity_usd: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawdownPoint {
    pub ts: String,
    pub drawdown_pct: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub ts: String,
    pub node_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Costs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_fees_usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_gas_usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_funding_usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_yield_usd: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultMetadata {
    pub policy: SimulationPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// First actual tick of the run (#167); may be later than the requested `start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_start: Option<String>,
    /// Last actual tick of the run (#167); may be earlier than the requested `end`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_end: Option<String>,
    #[serde(default)]
    pub data_coverage: Vec<Value>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestResult {
    #[serde(default = "default_result_schema_version")]
    pub schema_version: String,
    pub summary: Summary,
    #[serde(default)]
    pub equity_curve: Vec<EquityPoint>,
    #[serde(default)]
    pub drawdown_curve: Vec<DrawdownPoint>,
    #[serde(default)]
    pub trades: Vec<Trade>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_portfolio: Option<Portfolio>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub costs: Option<Costs>,
    pub metadata: ResultMetadata,
}

fn default_result_schema_version() -> String {
    "catalyst.backtest.result.v1".to_string()
}
