//! Resting limit-order semantics, shared by swaps and perps.
//!
//! A limit order is *placed* (validated against the ledger) but does not fill
//! until a later bar's price touches its limit. This module owns the two
//! decisions that are independent of the underlying instrument:
//!
//! - **direction**: is the order a buy or a sell? (Buys rest below the market and
//!   fill when price falls to them; sells rest above and fill when price rises.)
//! - **touch + fill price**: given a bar, does the order fill, and at what price?
//!   Filling is *gap-aware*: a bar that opens through the limit fills at the open
//!   (in the trader's favor); otherwise it fills exactly at the limit. A resting
//!   limit is a **maker** order: it fills at limit-or-better, with **no taker
//!   slippage and no AMM price impact** — no slippage model (including
//!   `amm_price_impact`) ever reprices the fill away from the limit (#162). The
//!   theoretical AMM impact price is attached to swap fills for honesty, never
//!   substituted.
//!
//! Placement is validated here; the resting book, expiry/TIF, and lifecycle
//! events live in the engine.

use rust_decimal::Decimal;

use catalyst_contracts::graph::{PerpSide, PerpOrderConfig, SwapConfig};
use catalyst_portfolio_ledger::{Ledger, PerpSide as LedgerPerpSide};

use crate::context::Bar;
use crate::pricing::{is_stable, parse};

/// Which way a resting order trades, relative to the base asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitSide {
    Buy,
    Sell,
}

impl LimitSide {
    pub fn as_str(self) -> &'static str {
        match self {
            LimitSide::Buy => "buy",
            LimitSide::Sell => "sell",
        }
    }
}

/// A validated limit order ready to rest in the engine's book.
#[derive(Debug, Clone)]
pub struct PlacedLimit {
    pub side: LimitSide,
    pub limit: Decimal,
    /// Venue/chain used for bar lookups while the order rests.
    pub venue: String,
    /// Base symbol used for bar lookups (the priced asset).
    pub symbol: String,
}

/// Outcome of validating a limit-order placement.
pub enum LimitPlacement {
    Placed(PlacedLimit),
    Rejected(String),
}

/// The fill price if `bar` touches `limit` for an order on `side`, else `None`.
///
/// Buy fills when the low reaches the limit; sell when the high reaches it.
/// Gap-through bars fill at the open (better for the trader).
pub fn limit_fill_price(bar: &Bar, side: LimitSide, limit: Decimal) -> Option<Decimal> {
    match side {
        LimitSide::Buy => (bar.low <= limit).then(|| bar.open.min(limit)),
        LimitSide::Sell => (bar.high >= limit).then(|| bar.open.max(limit)),
    }
}

fn limit_price(value: &Option<String>) -> Result<Decimal, String> {
    match value {
        Some(s) => {
            let d = parse(s);
            if d.is_zero() {
                Err(format!("invalid limit_price {s:?}"))
            } else {
                Ok(d)
            }
        }
        None => Err("limit order requires limit_price".to_string()),
    }
}

/// Validate a swap limit order against the ledger and resolve its side/base.
pub fn place_swap_limit(cfg: &SwapConfig) -> LimitPlacement {
    let (side, base) = match (is_stable(&cfg.from_asset), is_stable(&cfg.to_asset)) {
        (true, false) => (LimitSide::Buy, cfg.to_asset.as_str()),
        (false, true) => (LimitSide::Sell, cfg.from_asset.as_str()),
        _ => {
            return LimitPlacement::Rejected(format!(
                "unsupported swap {}->{}: exactly one side must be a stable asset",
                cfg.from_asset, cfg.to_asset
            ))
        }
    };
    let limit = match limit_price(&cfg.limit_price) {
        Ok(d) => d,
        Err(e) => return LimitPlacement::Rejected(e),
    };
    LimitPlacement::Placed(PlacedLimit {
        side,
        limit,
        venue: cfg.chain.clone(),
        symbol: base.to_string(),
    })
}

/// Validate a perp limit order against the ledger and resolve its side.
///
/// A reduce-only limit closes an existing position (take-profit), so it requires
/// one to exist; its direction is the closing direction of that position.
pub fn place_perp_limit(ledger: &Ledger, cfg: &PerpOrderConfig) -> LimitPlacement {
    let limit = match limit_price(&cfg.limit_price) {
        Ok(d) => d,
        Err(e) => return LimitPlacement::Rejected(e),
    };
    let side = if cfg.reduce_only {
        match ledger.perp(&cfg.chain, &cfg.symbol) {
            Some(p) => match p.side {
                LedgerPerpSide::Long => LimitSide::Sell,
                LedgerPerpSide::Short => LimitSide::Buy,
            },
            None => {
                return LimitPlacement::Rejected(format!(
                    "reduce-only limit for {} but no open position",
                    cfg.symbol
                ))
            }
        }
    } else {
        match cfg.side {
            PerpSide::Long => LimitSide::Buy,
            PerpSide::Short => LimitSide::Sell,
        }
    };
    LimitPlacement::Placed(PlacedLimit {
        side,
        limit,
        venue: cfg.chain.clone(),
        symbol: cfg.symbol.clone(),
    })
}
