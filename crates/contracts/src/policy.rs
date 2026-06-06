//! Simulation policy contract (simulation-policy.schema.json).

use serde::{Deserialize, Serialize};

use crate::Decimal;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BalancePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insufficient_balance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SlippagePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bps: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FeePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bps: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FillsPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_fills: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_selection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slippage: Option<SlippagePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fees: Option<FeePolicy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GasFallback {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GasPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<GasFallback>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SignalPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OrderingPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_tick: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DataPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_required: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_optional: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PerpPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquidation_check: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub funding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduce_only_validation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct YieldPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accrual: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulationPolicy {
    #[serde(default = "default_policy_schema_version")]
    pub schema_version: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<BalancePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fills: Option<FillsPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas: Option<GasPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signals: Option<SignalPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordering: Option<OrderingPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<DataPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perps: Option<PerpPolicy>,
    #[serde(rename = "yield", default, skip_serializing_if = "Option::is_none")]
    pub yield_: Option<YieldPolicy>,
}

fn default_policy_schema_version() -> String {
    "catalyst.backtest.policy.v1".to_string()
}
