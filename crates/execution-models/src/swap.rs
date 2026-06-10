//! EVM and Hyperliquid spot swap approximation.
//!
//! A swap converts `amount` of `from_asset` into `to_asset` on a venue. Exactly
//! one side must be a stable/quote asset; the other is the priced base. Buys
//! spend the stable amount as USD notional; sells dispose `amount` base units.

use rust_decimal::Decimal;

use catalyst_contracts::graph::SwapConfig;
use catalyst_portfolio_ledger::Ledger;
use catalyst_simulation_policies::{ResolvedPolicy, SlippageModel};

use crate::context::MarketContext;
use crate::outcome::{Execution, Fill};
use crate::pricing::{
    apply_bps, fee_usd, gas_usd, is_stable, parse, reference_price, slippage_bps, Direction,
};

/// Constant-product (x·y=k) average execution price including price impact, given
/// the trade `amount` and pool reserves. A buy spends `amount` quote and receives
/// `rb·amount/(rq+amount)` base (avg price `(rq+amount)/rb`); a sell disposes
/// `amount` base for `rq·amount/(rb+amount)` quote (avg price `rq/(rb+amount)`).
fn amm_price(dir: Direction, amount: Decimal, reserve_base: Decimal, reserve_quote: Decimal) -> Decimal {
    match dir {
        Direction::Buy => (reserve_quote + amount) / reserve_base,
        Direction::Sell => reserve_quote / (reserve_base + amount),
    }
}

/// Resolve a swap's trade direction and the priced base asset.
fn swap_direction(cfg: &SwapConfig) -> Result<(Direction, &str), String> {
    match (is_stable(&cfg.from_asset), is_stable(&cfg.to_asset)) {
        (true, false) => Ok((Direction::Buy, cfg.to_asset.as_str())),
        (false, true) => Ok((Direction::Sell, cfg.from_asset.as_str())),
        _ => Err(format!(
            "unsupported swap {}->{}: exactly one side must be a stable asset",
            cfg.from_asset, cfg.to_asset
        )),
    }
}

/// Resolve a swap's requested amount: the "all" sentinel spends the full balance;
/// relative amounts are pre-resolved to absolute by the engine before execution.
fn resolve_amount(ledger: &Ledger, cfg: &SwapConfig, venue: &str) -> Decimal {
    if cfg.amount.is_all() {
        ledger.balance(venue, &cfg.from_asset)
    } else {
        parse(cfg.amount.as_str())
    }
}

/// Depth-aware price impact (#40): under `amm_price_impact` with pool reserves,
/// fill at the constant-product average price (size-dependent); otherwise use
/// `fallback` (the market reference+slippage). **Market swaps only** — resting
/// limit fills are maker orders and are never repriced by the AMM model (#162);
/// see [`execute_swap_at`].
fn maybe_amm(
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    venue: &str,
    base: &str,
    dir: Direction,
    amount: Decimal,
    fallback: Decimal,
) -> Decimal {
    match (policy.slippage_model, ctx.pool_reserves(venue, base)) {
        (SlippageModel::AmmPriceImpact, Some((rb, rq))) if !rb.is_zero() && !rq.is_zero() => {
            amm_price(dir, amount, rb, rq)
        }
        _ => fallback,
    }
}

/// Market swap: fill at the current bar (reference price + model slippage).
pub fn execute_swap(
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    cfg: &SwapConfig,
) -> Execution {
    let venue = cfg.chain.as_str();
    let (dir, base) = match swap_direction(cfg) {
        Ok(x) => x,
        Err(e) => return Execution::rejected(e),
    };
    let bar = match ctx.bar(venue, base) {
        Some(b) => b,
        None => return Execution::rejected(format!("no price for {base} on {venue}")),
    };
    let amount = resolve_amount(ledger, cfg, venue);
    if amount.is_zero() {
        return Execution::rejected(format!("nothing to swap from {}", cfg.from_asset));
    }
    let next = ctx.next_bar(venue, base);
    let reference = reference_price(&bar, next.as_ref(), dir, policy);
    // Trade size in base units, for the volume model's participation rate.
    let base_amount = match dir {
        Direction::Buy if !reference.is_zero() => amount / reference,
        Direction::Buy => Decimal::ZERO,
        Direction::Sell => amount,
    };
    let reference_fill = apply_bps(reference, dir, slippage_bps(policy, base_amount, bar.volume));
    let price = maybe_amm(ctx, policy, venue, base, dir, amount, reference_fill);
    swap_at(ledger, ctx, policy, cfg, dir, base, amount, price)
}

/// Execute a swap at an explicit fill `price` (used by the engine's resting
/// limit-order fills). Direction and base are re-derived from the config.
///
/// Maker semantics (#162): the fill happens *exactly* at the engine-provided
/// price — the gap-aware limit-or-better price from `limit_fill_price`. The AMM
/// price-impact model is never applied here (it would reprice a buy limit at
/// 1900 to fill above 1900, violating limit-order semantics). For honesty,
/// under `amm_price_impact` with pool reserves present the theoretical
/// constant-product price is still computed and attached to the fill
/// (`amm_theoretical_price` / `amm_impact_exceeds_limit`), never substituted.
pub fn execute_swap_at(
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    cfg: &SwapConfig,
    price: Decimal,
) -> Execution {
    let venue = cfg.chain.as_str();
    let (dir, base) = match swap_direction(cfg) {
        Ok(x) => x,
        Err(e) => return Execution::rejected(e),
    };
    let amount = resolve_amount(ledger, cfg, venue);
    if amount.is_zero() {
        return Execution::rejected(format!("nothing to swap from {}", cfg.from_asset));
    }
    let theoretical = match (policy.slippage_model, ctx.pool_reserves(venue, base)) {
        (SlippageModel::AmmPriceImpact, Some((rb, rq))) if !rb.is_zero() && !rq.is_zero() => {
            Some(amm_price(dir, amount, rb, rq))
        }
        _ => None,
    };
    let mut out = swap_at(ledger, ctx, policy, cfg, dir, base, amount, price);
    if let (Execution::Executed(fill), Some(theo)) = (&mut out, theoretical) {
        fill.amm_theoretical_price = Some(theo);
        // Worse than the fill from the trader's perspective?
        fill.amm_impact_exceeds_limit = Some(match dir {
            Direction::Buy => theo > price,
            Direction::Sell => theo < price,
        });
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn swap_at(
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    cfg: &SwapConfig,
    dir: Direction,
    base: &str,
    amount: Decimal,
    price: Decimal,
) -> Execution {
    let venue = cfg.chain.as_str();
    if price.is_zero() {
        return Execution::rejected(format!("zero price for {base} on {venue}"));
    }
    // A non-positive swap amount can only mean an empty or overdrawn source
    // balance (e.g. `amount: "all"` on a negative balance under the
    // `allow_negative` policy) — reject it here so the buy/sell credits below
    // are provably non-negative (#165).
    if amount <= Decimal::ZERO {
        return Execution::rejected(format!("nothing to swap from {}", cfg.from_asset));
    }
    let gas = gas_usd(venue, ctx, policy);

    match dir {
        Direction::Buy => {
            let notional = amount; // stable amount == USD notional
            let fee = fee_usd(notional, policy);
            let received = notional / price;
            let total_out = notional + fee + gas;
            if let Err(e) = ledger.debit(venue, &cfg.from_asset, total_out) {
                return Execution::rejected(e.to_string());
            }
            ledger
                .credit(venue, &cfg.to_asset, received)
                .expect("non-negative by construction (amount > 0 guarded above)");
            ledger.record_fee(fee);
            ledger.record_gas(gas);
            Execution::Executed(Fill {
                kind: "swap".into(),
                venue: venue.into(),
                symbol: Some(base.into()),
                side: Some("buy".into()),
                price: Some(price),
                amount: Some(received),
                value_usd: Some(notional),
                fee_usd: fee,
                gas_usd: gas,
                realized_pnl_usd: None,
                amm_theoretical_price: None,
                amm_impact_exceeds_limit: None,
            })
        }
        Direction::Sell => {
            let proceeds = amount * price;
            let fee = fee_usd(proceeds, policy);
            let net: Decimal = proceeds - fee - gas;
            // Reject (don't credit a negative `net`) when fee + gas swallow the
            // proceeds — e.g. dust sold on an EVM chain where gas exceeds the
            // trade value. Crediting a negative amount would mint phantom debt in
            // the destination asset. Checked before debiting so the ledger is
            // left untouched on rejection.
            if net <= Decimal::ZERO {
                return Execution::rejected(format!(
                    "swap proceeds {proceeds} do not cover fee {fee} + gas {gas}"
                ));
            }
            if let Err(e) = ledger.debit(venue, &cfg.from_asset, amount) {
                return Execution::rejected(e.to_string());
            }
            ledger
                .credit(venue, &cfg.to_asset, net)
                .expect("non-negative by construction (net > 0 guarded above)");
            ledger.record_fee(fee);
            ledger.record_gas(gas);
            Execution::Executed(Fill {
                kind: "swap".into(),
                venue: venue.into(),
                symbol: Some(base.into()),
                side: Some("sell".into()),
                price: Some(price),
                amount: Some(amount),
                value_usd: Some(proceeds),
                fee_usd: fee,
                gas_usd: gas,
                realized_pnl_usd: None,
                amm_theoretical_price: None,
                amm_impact_exceeds_limit: None,
            })
        }
    }
}
