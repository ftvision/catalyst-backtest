//! EVM and Hyperliquid spot swap approximation.
//!
//! A swap converts `amount` of `from_asset` into `to_asset` on a venue. Exactly
//! one side must be a stable/quote asset; the other is the priced base. Buys
//! spend the stable amount as USD notional; sells dispose `amount` base units.

use rust_decimal::Decimal;

use catalyst_contracts::graph::SwapConfig;
use catalyst_portfolio_ledger::Ledger;
use catalyst_simulation_policies::ResolvedPolicy;

use crate::context::MarketContext;
use crate::outcome::{Execution, Fill};
use crate::pricing::{fee_usd, fill_price, gas_usd, is_stable, parse, Direction};

pub fn execute_swap(
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    cfg: &SwapConfig,
) -> Execution {
    let venue = cfg.chain.as_str();
    let (dir, base) = match (is_stable(&cfg.from_asset), is_stable(&cfg.to_asset)) {
        (true, false) => (Direction::Buy, cfg.to_asset.as_str()),
        (false, true) => (Direction::Sell, cfg.from_asset.as_str()),
        _ => {
            return Execution::rejected(format!(
                "unsupported swap {}->{}: exactly one side must be a stable asset",
                cfg.from_asset, cfg.to_asset
            ))
        }
    };

    let bar = match ctx.bar(venue, base) {
        Some(b) => b,
        None => return Execution::rejected(format!("no price for {base} on {venue}")),
    };
    let price = fill_price(&bar, dir, policy);
    if price.is_zero() {
        return Execution::rejected(format!("zero price for {base} on {venue}"));
    }

    // Resolve the requested amount (supporting the "all" sentinel).
    let amount = if cfg.amount == "all" {
        ledger.balance(venue, &cfg.from_asset)
    } else {
        parse(&cfg.amount)
    };
    if amount.is_zero() {
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
            ledger.credit(venue, &cfg.to_asset, received);
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
            })
        }
        Direction::Sell => {
            let proceeds = amount * price;
            let fee = fee_usd(proceeds, policy);
            if let Err(e) = ledger.debit(venue, &cfg.from_asset, amount) {
                return Execution::rejected(e.to_string());
            }
            let net: Decimal = proceeds - fee - gas;
            ledger.credit(venue, &cfg.to_asset, net);
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
            })
        }
    }
}
