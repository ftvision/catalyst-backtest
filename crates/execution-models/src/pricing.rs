//! Shared pricing helpers: price selection, slippage, fees, and gas, all driven
//! by the resolved policy.

use rust_decimal::Decimal;

use catalyst_simulation_policies::{
    FeeModel, GasModel, PriceSelection, ResolvedPolicy, SlippageModel,
};

use crate::context::{Bar, MarketContext};

const BPS: i64 = 10_000;

/// Assets treated as cash/quote (USD-equivalent).
pub fn is_stable(asset: &str) -> bool {
    matches!(asset.to_ascii_uppercase().as_str(), "USDC" | "USDT" | "USD" | "DAI" | "USDC.E")
}

/// Trade side relative to the base asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Buy,
    Sell,
}

/// Pick the reference price per the policy's price-selection rule.
///
/// `next_open` uses the *next* bar's open (no look-ahead at the current close);
/// it falls back to the current close only when there is no next bar (the final
/// bar of the run), which is the one case where a next open cannot exist.
pub fn reference_price(
    bar: &Bar,
    next: Option<&Bar>,
    dir: Direction,
    policy: &ResolvedPolicy,
) -> Decimal {
    match policy.price_selection {
        PriceSelection::Close => bar.close,
        PriceSelection::NextOpen => next.map(|b| b.open).unwrap_or(bar.close),
        PriceSelection::Open => bar.open,
        PriceSelection::Mid => (bar.high + bar.low) / Decimal::TWO,
        PriceSelection::WorseSideOhlc => match dir {
            Direction::Buy => bar.high,
            Direction::Sell => bar.low,
        },
    }
}

/// Apply slippage adverse to the trader: buys fill higher, sells fill lower.
pub fn apply_slippage(price: Decimal, dir: Direction, policy: &ResolvedPolicy) -> Decimal {
    let bps = match policy.slippage_model {
        SlippageModel::FixedBps => parse(&policy.slippage_bps),
        SlippageModel::VolumeBased | SlippageModel::None => Decimal::ZERO,
    };
    let factor = bps / Decimal::from(BPS);
    match dir {
        Direction::Buy => price * (Decimal::ONE + factor),
        Direction::Sell => price * (Decimal::ONE - factor),
    }
}

/// The fill price: reference price (current + optional next bar) plus slippage.
pub fn fill_price(
    bar: &Bar,
    next: Option<&Bar>,
    dir: Direction,
    policy: &ResolvedPolicy,
) -> Decimal {
    apply_slippage(reference_price(bar, next, dir, policy), dir, policy)
}

/// Trading fee in USD on a notional.
pub fn fee_usd(notional_usd: Decimal, policy: &ResolvedPolicy) -> Decimal {
    match policy.fee_model {
        FeeModel::FixedBps => notional_usd * parse(&policy.fee_bps) / Decimal::from(BPS),
        FeeModel::VenueFeeTable | FeeModel::None => Decimal::ZERO,
    }
}

/// Gas in USD for one on-chain action on `venue`. Hyperliquid carries no EVM gas.
pub fn gas_usd(venue: &str, ctx: &dyn MarketContext, policy: &ResolvedPolicy) -> Decimal {
    if venue == "hyperliquid" {
        return Decimal::ZERO;
    }
    match policy.gas_model {
        GasModel::None => Decimal::ZERO,
        GasModel::FixedUsd | GasModel::FixedNative => parse(&policy.gas_fixed_amount),
        GasModel::HistoricalFeeHistory => {
            ctx.gas_usd(venue).unwrap_or_else(|| parse(&policy.gas_fixed_amount))
        }
    }
}

/// Parse a decimal string, defaulting to zero on malformed input (policy values
/// are validated upstream by the policy crate).
pub fn parse(s: &str) -> Decimal {
    s.parse().unwrap_or(Decimal::ZERO)
}
