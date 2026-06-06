//! Perp and yield position models.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use catalyst_contracts::trace as ct;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerpSide {
    Long,
    Short,
}

impl PerpSide {
    pub fn as_str(self) -> &'static str {
        match self {
            PerpSide::Long => "long",
            PerpSide::Short => "short",
        }
    }
}

/// An open perpetual position. `size` is in base units (always non-negative);
/// direction is carried by `side`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerpPosition {
    pub venue: String,
    pub symbol: String,
    pub side: PerpSide,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub leverage: Decimal,
    pub margin_usd: Decimal,
}

impl PerpPosition {
    pub fn key(&self) -> (String, String) {
        (self.venue.clone(), self.symbol.clone())
    }

    /// Notional value at entry (`size * entry_price`).
    pub fn notional(&self) -> Decimal {
        self.size * self.entry_price
    }

    /// Unrealized PnL at `mark` price (USD).
    pub fn unrealized_pnl(&self, mark: Decimal) -> Decimal {
        match self.side {
            PerpSide::Long => (mark - self.entry_price) * self.size,
            PerpSide::Short => (self.entry_price - mark) * self.size,
        }
    }

    pub fn to_contract(&self) -> ct::PerpPosition {
        ct::PerpPosition {
            venue: self.venue.clone(),
            symbol: self.symbol.clone(),
            side: self.side.as_str().to_string(),
            size: self.size.normalize().to_string(),
            entry_price: self.entry_price.normalize().to_string(),
            leverage: Some(self.leverage.normalize().to_string()),
            margin_usd: Some(self.margin_usd.normalize().to_string()),
            liquidation_price: None,
        }
    }
}

/// A yield position (e.g. an Aave aToken balance), tracked as principal plus
/// accrued interest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YieldPosition {
    pub protocol: String,
    pub pool: Option<String>,
    pub asset: String,
    pub chain: String,
    pub principal: Decimal,
    pub accrued: Decimal,
}

pub type YieldKey = (String, String, String, Option<String>);

impl YieldPosition {
    pub fn key(&self) -> YieldKey {
        (self.protocol.clone(), self.asset.clone(), self.chain.clone(), self.pool.clone())
    }

    /// Total redeemable value (principal + accrued).
    pub fn value(&self) -> Decimal {
        self.principal + self.accrued
    }

    pub fn to_contract(&self) -> ct::YieldPosition {
        ct::YieldPosition {
            protocol: self.protocol.clone(),
            asset: self.asset.clone(),
            chain: self.chain.clone(),
            pool: self.pool.clone(),
            principal: self.principal.normalize().to_string(),
            accrued: Some(self.accrued.normalize().to_string()),
        }
    }
}
