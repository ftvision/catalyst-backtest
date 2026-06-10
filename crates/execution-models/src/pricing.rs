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

/// Effective slippage in bps for the active model. `fixed_bps` (and the
/// `amm_price_impact` fallback) use the configured bps; `volume_based` scales it
/// by participation `p = base_amount / bar_volume` via the square-root law
/// (`bps + coef·√p`, where `coef` is the policy's `volume_impact_coef_bps`
/// knob — extra bps at 100% participation, #169), falling back to the
/// configured bps when the bar has no volume; `none` is zero.
/// See docs/logic/slippage-models.md.
pub fn slippage_bps(policy: &ResolvedPolicy, base_amount: Decimal, bar_volume: Option<Decimal>) -> Decimal {
    let base = parse_policy("slippage_bps", &policy.slippage_bps);
    match policy.slippage_model {
        // amm_price_impact's depth model is applied from pool reserves in the swap
        // path; where reserves don't apply (perps, or swaps without a reserves
        // series) it falls back to the configured bps rather than charging nothing.
        SlippageModel::FixedBps | SlippageModel::AmmPriceImpact => base,
        SlippageModel::VolumeBased => match bar_volume {
            Some(v) if v > Decimal::ZERO && base_amount > Decimal::ZERO => {
                base + parse_policy("volume_impact_coef_bps", &policy.volume_impact_coef_bps)
                    * dec_sqrt(base_amount / v)
            }
            // No (or zero) volume -> fall back to fixed bps, never silently zero.
            _ => base,
        },
        SlippageModel::None => Decimal::ZERO,
    }
}

/// Apply a bps haircut adverse to the trader: buys fill higher, sells fill lower.
pub fn apply_bps(price: Decimal, dir: Direction, bps: Decimal) -> Decimal {
    let factor = bps / Decimal::from(BPS);
    match dir {
        Direction::Buy => price * (Decimal::ONE + factor),
        Direction::Sell => price * (Decimal::ONE - factor),
    }
}

/// √ on `Decimal` via an f64 round-trip (deterministic; precise enough for a
/// slippage estimate).
fn dec_sqrt(x: Decimal) -> Decimal {
    use rust_decimal::prelude::*;
    let f = x.to_f64().unwrap_or(0.0).max(0.0).sqrt();
    Decimal::from_f64_retain(f).unwrap_or(Decimal::ZERO)
}

/// Trading fee in USD on a notional.
pub fn fee_usd(notional_usd: Decimal, policy: &ResolvedPolicy) -> Decimal {
    match policy.fee_model {
        FeeModel::FixedBps => {
            notional_usd * parse_policy("fee_bps", &policy.fee_bps) / Decimal::from(BPS)
        }
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
        GasModel::FixedUsd | GasModel::FixedNative => {
            parse_policy("gas_fixed_amount", &policy.gas_fixed_amount)
        }
        GasModel::HistoricalFeeHistory => ctx
            .gas_usd(venue)
            .unwrap_or_else(|| parse_policy("gas_fixed_amount", &policy.gas_fixed_amount)),
    }
}

/// Parse a graph-config decimal string (limit prices, absolute swap/yield
/// amounts, perp sizes), defaulting to zero on malformed input; callers treat
/// the resulting zero as invalid and reject it explicitly downstream. Relative
/// sizing values and perp `"all"` are validated strictly at graph compile time
/// (#160), so this leniency now covers only absolute decimal fields. Policy
/// values must use [`parse_policy`].
pub fn parse(s: &str) -> Decimal {
    s.parse().unwrap_or(Decimal::ZERO)
}

/// Parse a policy decimal that `validate` has already guaranteed parseable.
/// A failure here is an engine bug (a caller bypassed policy validation), so it is loud.
pub(crate) fn parse_policy(field: &str, s: &str) -> Decimal {
    s.parse().unwrap_or_else(|_| {
        panic!("policy {field} = {s:?} failed to parse; policy validation should have rejected this")
    })
}
