//! Aave-style yield deposit/withdraw approximation.
//!
//! Deposits move principal from the chain balance into a yield position; the
//! interest itself accrues tick-by-tick in the engine. Gas is charged in USD on
//! the chain balance.
//!
//! These are the one pair of models with two fallible balance moves (the
//! principal move and a separate gas debit). The engine executes every action on
//! a trial copy of the ledger and only commits it on success, so a partway
//! failure here is discarded wholesale — no manual rollback is needed.

use rust_decimal::Decimal;

use catalyst_contracts::graph::YieldConfig;
use catalyst_portfolio_ledger::Ledger;
use catalyst_simulation_policies::ResolvedPolicy;

use crate::context::MarketContext;
use crate::outcome::{Execution, Fill};
use crate::pricing::{gas_usd, is_stable, parse};

/// USD price of one unit of `asset` on `chain` (1 for stablecoins). `None` when a
/// non-stable asset has no price this tick — the deposit/withdraw can't be valued
/// or its gas converted, so the caller rejects.
fn asset_price(ctx: &dyn MarketContext, chain: &str, asset: &str) -> Option<Decimal> {
    if is_stable(asset) {
        Some(Decimal::ONE)
    } else {
        ctx.bar(chain, asset).map(|b| b.close).filter(|p| !p.is_zero())
    }
}

pub fn execute_yield_deposit(
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    cfg: &YieldConfig,
) -> Execution {
    let chain = cfg.chain.as_str();
    let pool = cfg.pool.as_deref();
    let gas = gas_usd(chain, ctx, policy);
    // Gas is a USD fee but is paid from the (asset-denominated) chain balance, so
    // convert it into asset units before touching the balance (#115). For a USD
    // stablecoin this is the same number; for e.g. ETH it is gas_usd / ETH price.
    let Some(price) = asset_price(ctx, chain, &cfg.asset) else {
        return Execution::rejected(format!("no price for {} on {chain}", cfg.asset));
    };
    let gas_asset = gas / price;

    let amount = if cfg.amount.is_all() {
        // Reserve gas (in asset units) so the deposit leaves enough to pay for it.
        (ledger.balance(chain, &cfg.asset) - gas_asset).max(Decimal::ZERO)
    } else {
        parse(cfg.amount.as_str())
    };
    if amount.is_zero() {
        return Execution::rejected(format!("nothing to deposit for {}", cfg.asset));
    }

    if let Err(e) = ledger.deposit_yield(&cfg.protocol, &cfg.asset, chain, pool, amount) {
        return Execution::rejected(e.to_string());
    }
    if let Err(e) = ledger.debit(chain, &cfg.asset, gas_asset) {
        return Execution::rejected(e.to_string());
    }
    ledger.record_gas(gas);

    Execution::Executed(Fill {
        kind: "yield_deposit".into(),
        venue: chain.into(),
        symbol: Some(cfg.asset.clone()),
        side: Some("deposit".into()),
        price: None,
        amount: Some(amount),
        value_usd: Some(amount * price),
        fee_usd: Decimal::ZERO,
        gas_usd: gas,
        realized_pnl_usd: None,
    })
}

pub fn execute_yield_withdraw(
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    cfg: &YieldConfig,
) -> Execution {
    let chain = cfg.chain.as_str();
    let pool = cfg.pool.as_deref();
    let gas = gas_usd(chain, ctx, policy);
    // Gas (USD) is paid from the asset-denominated chain balance — convert to asset
    // units (#115); for a stablecoin this is unchanged.
    let Some(price) = asset_price(ctx, chain, &cfg.asset) else {
        return Execution::rejected(format!("no price for {} on {chain}", cfg.asset));
    };
    let gas_asset = gas / price;

    let amount = if cfg.amount.is_all() {
        ledger.yield_value(&cfg.protocol, &cfg.asset, chain, pool)
    } else {
        parse(cfg.amount.as_str())
    };
    if amount.is_zero() {
        return Execution::rejected(format!("nothing to withdraw for {}", cfg.asset));
    }

    if let Err(e) = ledger.withdraw_yield(&cfg.protocol, &cfg.asset, chain, pool, amount) {
        return Execution::rejected(e.to_string());
    }
    if let Err(e) = ledger.debit(chain, &cfg.asset, gas_asset) {
        return Execution::rejected(e.to_string());
    }
    ledger.record_gas(gas);

    Execution::Executed(Fill {
        kind: "yield_withdraw".into(),
        venue: chain.into(),
        symbol: Some(cfg.asset.clone()),
        side: Some("withdraw".into()),
        price: None,
        amount: Some(amount),
        value_usd: Some(amount * price),
        fee_usd: Decimal::ZERO,
        gas_usd: gas,
        realized_pnl_usd: None,
    })
}
