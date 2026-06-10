//! Result of attempting to execute an action.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A successful fill, with everything the engine needs to log a trade event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    /// swap / perp_open / perp_close / yield_deposit / yield_withdraw
    pub kind: String,
    pub venue: String,
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
    pub fee_usd: Decimal,
    pub gas_usd: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realized_pnl_usd: Option<Decimal>,
    /// Honesty fields for resting limit fills under `amm_price_impact` (#162):
    /// the theoretical constant-product average price the trade *would* have
    /// paid as a taker against the pool reserves. Informational only — the fill
    /// price is always the maker (limit-or-better) price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amm_theoretical_price: Option<Decimal>,
    /// True when the theoretical AMM price is worse than the actual fill price
    /// from the trader's perspective (theoretical > fill for buys, < for sells).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amm_impact_exceeds_limit: Option<bool>,
}

/// Outcome of an execution attempt. A rejection leaves the ledger unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Execution {
    Executed(Fill),
    Rejected { reason: String },
}

impl Execution {
    pub fn rejected(reason: impl Into<String>) -> Self {
        Execution::Rejected { reason: reason.into() }
    }

    pub fn is_executed(&self) -> bool {
        matches!(self, Execution::Executed(_))
    }

    pub fn fill(&self) -> Option<&Fill> {
        match self {
            Execution::Executed(f) => Some(f),
            Execution::Rejected { .. } => None,
        }
    }
}
