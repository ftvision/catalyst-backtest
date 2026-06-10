//! Hyperliquid perp open/add and reduce-only close approximation.

use rust_decimal::Decimal;

use catalyst_contracts::graph::{PerpOrderConfig, PerpSide as CfgSide};
use catalyst_portfolio_ledger::{Ledger, PerpPosition, PerpSide};
use catalyst_simulation_policies::ResolvedPolicy;

use crate::context::MarketContext;
use crate::outcome::{Execution, Fill};
use crate::pricing::{apply_bps, fee_usd, parse, reference_price, slippage_bps, Direction};

fn ledger_side(side: CfgSide) -> PerpSide {
    match side {
        CfgSide::Long => PerpSide::Long,
        CfgSide::Short => PerpSide::Short,
    }
}

/// Market perp order: fill at the current bar (reference price + slippage).
pub fn execute_perp(
    ledger: &mut Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    cfg: &PerpOrderConfig,
) -> Execution {
    let venue = cfg.chain.as_str();
    let bar = match ctx.bar(venue, &cfg.symbol) {
        Some(b) => b,
        None => return Execution::rejected(format!("no price for {} on {venue}", cfg.symbol)),
    };
    let dir = if cfg.reduce_only {
        // Closing direction depends on the side of the position being reduced.
        match ledger.perp(venue, &cfg.symbol) {
            Some(p) => match p.side {
                PerpSide::Long => Direction::Sell,
                PerpSide::Short => Direction::Buy,
            },
            None => {
                return Execution::rejected(format!(
                    "reduce-only order for {} but no open position",
                    cfg.symbol
                ))
            }
        }
    } else {
        match ledger_side(cfg.side.clone()) {
            PerpSide::Long => Direction::Buy,
            PerpSide::Short => Direction::Sell,
        }
    };
    let next = ctx.next_bar(venue, &cfg.symbol);
    let reference = reference_price(&bar, next.as_ref(), dir, policy);
    // Trade size in base units (notional / reference price), for the volume
    // model's participation rate. amm_price_impact is swap-only, so perps only
    // ever see the bps-based models here.
    let base_amount = if reference.is_zero() {
        Decimal::ZERO
    } else {
        parse(cfg.size_usd.as_str()) / reference
    };
    let price = apply_bps(reference, dir, slippage_bps(policy, base_amount, bar.volume));
    execute_perp_at(ledger, policy, cfg, price)
}

/// Execute a perp order at an explicit fill `price` (used by both the market path
/// above, after price selection, and the engine's resting limit-order fills).
pub fn execute_perp_at(
    ledger: &mut Ledger,
    policy: &ResolvedPolicy,
    cfg: &PerpOrderConfig,
    price: Decimal,
) -> Execution {
    if price.is_zero() {
        return Execution::rejected(format!("zero price for {}", cfg.symbol));
    }
    if cfg.reduce_only {
        close_perp(ledger, policy, cfg, price)
    } else {
        open_perp(ledger, policy, cfg, price)
    }
}

fn open_perp(
    ledger: &mut Ledger,
    policy: &ResolvedPolicy,
    cfg: &PerpOrderConfig,
    price: Decimal,
) -> Execution {
    let venue = cfg.chain.as_str();
    let side = ledger_side(cfg.side.clone());

    let notional = parse(cfg.size_usd.as_str());
    let leverage = cfg.leverage.as_deref().map(parse).filter(|l| !l.is_zero()).unwrap_or(Decimal::ONE);
    let margin = notional / leverage;
    let size_base = notional / price;
    let fee = fee_usd(notional, policy);

    // Net against any existing position on this (venue, symbol).
    let merged = match ledger.perp(venue, &cfg.symbol) {
        Some(existing) if existing.side == side => {
            let new_size = existing.size + size_base;
            let weighted_entry =
                (existing.entry_price * existing.size + price * size_base) / new_size;
            Some(PerpPosition {
                venue: venue.into(),
                symbol: cfg.symbol.clone(),
                side,
                size: new_size,
                entry_price: weighted_entry,
                leverage,
                margin_usd: existing.margin_usd + margin,
            })
        }
        Some(_) => {
            return Execution::rejected(
                "cannot add opposite-side perp without reduce_only".to_string(),
            )
        }
        None => None,
    };

    // Margin + fee must be available as USDC collateral. Unlike the close path,
    // the open fee can never be forgiven: it is debited up front and the open is
    // rejected if unfunded, so `record_fee(fee)` here always equals cash paid.
    if let Err(e) = ledger.debit(venue, "USDC", margin + fee) {
        return Execution::rejected(e.to_string());
    }
    ledger.record_fee(fee);
    match merged {
        Some(position) => ledger.set_perp(position),
        None => ledger.set_perp(PerpPosition {
            venue: venue.into(),
            symbol: cfg.symbol.clone(),
            side,
            size: size_base,
            entry_price: price,
            leverage,
            margin_usd: margin,
        }),
    }

    Execution::Executed(Fill {
        kind: "perp_open".into(),
        venue: venue.into(),
        symbol: Some(cfg.symbol.clone()),
        side: Some(side.as_str().into()),
        price: Some(price),
        amount: Some(size_base),
        value_usd: Some(notional),
        fee_usd: fee,
        gas_usd: Decimal::ZERO,
        realized_pnl_usd: None,
        amm_theoretical_price: None,
        amm_impact_exceeds_limit: None,
    })
}

fn close_perp(
    ledger: &mut Ledger,
    policy: &ResolvedPolicy,
    cfg: &PerpOrderConfig,
    price: Decimal,
) -> Execution {
    let venue = cfg.chain.as_str();
    let position = match ledger.perp(venue, &cfg.symbol) {
        Some(p) => p.clone(),
        None => {
            return Execution::rejected(format!(
                "reduce-only order for {} but no open position",
                cfg.symbol
            ))
        }
    };

    // Size the close by the position's entry price so a reduce-only order whose
    // size_usd matches the opened notional closes the whole position regardless
    // of where the mark has moved (clamped to the open size).
    let requested_base = parse(cfg.size_usd.as_str()) / position.entry_price;
    let close_base = requested_base.min(position.size);
    let fraction = close_base / position.size;

    let pnl_per_base = match position.side {
        PerpSide::Long => price - position.entry_price,
        PerpSide::Short => position.entry_price - price,
    };
    let realized_pnl = pnl_per_base * close_base;
    let returned_margin = position.margin_usd * fraction;
    let notional_closed = close_base * price;
    let fee = fee_usd(notional_closed, policy);
    // A position's loss can't exceed the margin posted for the closed fraction:
    // floor the settlement at zero so an underwater close returns nothing rather
    // than crediting a negative amount (which would claw back unposted collateral
    // the trader never deposited). Beyond this the position is bankrupt — see
    // liquidation handling.
    //
    // Fee reconciliation (#165): the zero floor means the venue can only take
    // its fee out of whatever gross value (`returned_margin + realized_pnl`)
    // the close actually returns. On a bankrupt close (`gross <= 0`) the fee is
    // forgiven in cash, and on a partial-coverage close (`0 < gross < fee`)
    // only `gross` of it is collected — so the *recorded* fee (and the fill's
    // `fee_usd`) is the collected amount, keeping `fees_usd` reconciled with
    // actual cash movement. `realized_pnl_usd` stays the full economic PnL
    // (a P&L statistic, not a cash flow).
    let gross = returned_margin + realized_pnl;
    let fee_collected = fee.min(gross.max(Decimal::ZERO));
    let settlement = (gross - fee).max(Decimal::ZERO);

    if close_base == position.size {
        ledger
            .close_perp(venue, &cfg.symbol, settlement)
            .expect("position exists; settlement floored non-negative");
    } else {
        let mut reduced = position.clone();
        reduced.size -= close_base;
        reduced.margin_usd -= returned_margin;
        ledger.set_perp(reduced);
        ledger
            .credit(venue, "USDC", settlement)
            .expect("non-negative by construction (floored above)");
    }
    ledger.record_fee(fee_collected);

    Execution::Executed(Fill {
        kind: "perp_close".into(),
        venue: venue.into(),
        symbol: Some(cfg.symbol.clone()),
        side: Some(cfg.side.as_str_close()),
        price: Some(price),
        amount: Some(close_base),
        value_usd: Some(notional_closed),
        fee_usd: fee_collected,
        gas_usd: Decimal::ZERO,
        realized_pnl_usd: Some(realized_pnl),
        amm_theoretical_price: None,
        amm_impact_exceeds_limit: None,
    })
}

// Small helper to render the closing side label from the config side.
trait SideLabel {
    fn as_str_close(&self) -> String;
}
impl SideLabel for CfgSide {
    fn as_str_close(&self) -> String {
        match self {
            CfgSide::Long => "long".to_string(),
            CfgSide::Short => "short".to_string(),
        }
    }
}
