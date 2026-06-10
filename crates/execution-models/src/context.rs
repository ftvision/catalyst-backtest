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
    /// Traded volume in base units for the bar, when the source provides it.
    /// `None` for sources that don't (e.g. Dune-derived candles); the volume
    /// slippage model falls back to fixed bps when it's absent.
    pub volume: Option<Decimal>,
}

impl Bar {
    pub fn flat(price: Decimal) -> Self {
        Bar { open: price, high: price, low: price, close: price, volume: None }
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

    /// Marking close for a (venue, symbol): the price *sizing* uses to convert
    /// a USD slice into asset units (#119(d)). The engine overrides this with
    /// the same bounded, venue-scoped carry-forward (`close_at`) that equity
    /// valuation uses, so sizing and equity never disagree about whether an
    /// asset is priced. The default — the current tick's exact bar close —
    /// keeps simple implementations (test fakes) on exact-bar behavior.
    ///
    /// Note this is a *marking* price, not a fillable one: execution models
    /// still require an exact bar to fill (`swap.rs`'s "no price" guard) or to
    /// convert real money (`yields.rs`'s exact-bar gate, #115).
    fn mark_close(&self, venue: &str, symbol: &str) -> Option<Decimal> {
        self.bar(venue, symbol).map(|b| b.close)
    }

    /// Gas cost in USD for a single on-chain action on `chain` (None if unknown).
    fn gas_usd(&self, chain: &str) -> Option<Decimal>;

    /// AMM pool reserves `(reserve_base, reserve_quote)` for a (venue, symbol) at
    /// the current tick, when a depth/liquidity series is available — used by the
    /// `amm_price_impact` slippage model. None when no pool data is present.
    fn pool_reserves(&self, _venue: &str, _symbol: &str) -> Option<(Decimal, Decimal)> {
        None
    }
}
