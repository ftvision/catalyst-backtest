//! What an execution model needs to know about the market at a tick.
//!
//! The engine implements [`MarketContext`] over the normalized market data
//! bundle for the current tick; execution models read prices/gas through it and
//! never touch raw data.

use rust_decimal::Decimal;

/// One OHLC bar for a (venue, symbol) at the current tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
}

impl Bar {
    pub fn flat(price: Decimal) -> Self {
        Bar { open: price, high: price, low: price, close: price }
    }
}

/// Read-only market view for the current tick.
pub trait MarketContext {
    /// Current OHLC bar for a tradeable symbol on a venue.
    fn bar(&self, venue: &str, symbol: &str) -> Option<Bar>;

    /// The *next* bar after the current tick (None on the final bar). Used for
    /// honest `next_open` fills — an order decided on this bar fills at the next
    /// bar's open, avoiding the look-ahead of filling at this bar's close.
    fn next_bar(&self, _venue: &str, _symbol: &str) -> Option<Bar> {
        None
    }

    /// Gas cost in USD for a single on-chain action on `chain` (None if unknown).
    fn gas_usd(&self, chain: &str) -> Option<Decimal>;
}
