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
//!
//! Placement also computes the order's **reservation** (#124): the exact amount
//! the eventual fill will debit, earmarked in the ledger while the order rests
//! so other actions cannot spend it (and validated up front, so an unaffordable
//! order is rejected at placement instead of resting doomed). Reservation
//! amounts are price-independent — a maker fill never pays slippage (#162) —
//! and per order type:
//!
//! | Order | Fill debits | Reserved |
//! | --- | --- | --- |
//! | Swap buy (stable→asset) | `amount + fee + gas` of `from_asset` | the same sum (gas estimated at the placement bar) |
//! | Swap sell (asset→stable) | exactly `amount` base units | `amount` |
//! | Perp limit open | `margin + fee` USDC (`margin = size_usd/leverage`) | the same sum |
//! | Perp reduce-only limit | nothing (credits only) | nothing |
//!
//! The one drift vector is historical gas between placement and fill — a
//! fill-time shortfall is rejected loudly (engine pushes a run-level warning).

use rust_decimal::Decimal;

use catalyst_contracts::graph::{PerpSide, PerpOrderConfig, SwapConfig};
use catalyst_portfolio_ledger::{Ledger, PerpSide as LedgerPerpSide};
use catalyst_simulation_policies::ResolvedPolicy;

use crate::context::{Bar, MarketContext};
use crate::pricing::{fee_usd, gas_usd, is_stable, parse};

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
    /// What the eventual fill will debit, earmarked while the order rests
    /// (#124): `(asset, amount)` on `venue`. `None` for credit-only orders
    /// (a perp reduce-only close) and zero-spend placements.
    pub reservation: Option<(String, Decimal)>,
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

/// The exact spend a swap fill will debit from `from_asset` (#124):
/// buy = `amount + fee + gas` (gas estimated at the current bar), sell =
/// exactly `amount`. `None` when the pair has no stable side or the amount is
/// non-positive (the fill rejects those itself; nothing to earmark). The
/// engine resolves relative amounts — and freezes the `"all"` sentinel — at
/// the decision bar before queuing, so `cfg.amount` is an absolute decimal
/// here.
pub fn swap_reservation(
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    cfg: &SwapConfig,
) -> Option<(String, Decimal)> {
    let dir_buy = match (is_stable(&cfg.from_asset), is_stable(&cfg.to_asset)) {
        (true, false) => true,
        (false, true) => false,
        _ => return None,
    };
    let amount = parse(cfg.amount.as_str());
    if amount <= Decimal::ZERO {
        return None;
    }
    let spend = if dir_buy {
        amount + fee_usd(amount, policy) + gas_usd(&cfg.chain, ctx, policy)
    } else {
        amount
    };
    Some((cfg.from_asset.clone(), spend))
}

/// The exact spend a perp order's fill will debit from the venue's USDC
/// (#124): `margin + fee` for an open (`margin = size_usd / leverage`,
/// leverage defaulting to 1 like the fill path). `None` for reduce-only
/// orders (credits only) and non-positive sizes.
pub fn perp_reservation(policy: &ResolvedPolicy, cfg: &PerpOrderConfig) -> Option<(String, Decimal)> {
    if cfg.reduce_only {
        return None;
    }
    let notional = parse(cfg.size_usd.as_str());
    if notional <= Decimal::ZERO {
        return None;
    }
    let leverage =
        cfg.leverage.as_deref().map(parse).filter(|l| !l.is_zero()).unwrap_or(Decimal::ONE);
    let margin = notional / leverage;
    Some(("USDC".to_string(), margin + fee_usd(notional, policy)))
}

/// Validate that `venue` has `amount` of `asset` *available* (balance minus
/// existing reservations) to back a new reservation. Always passes under
/// `allow_negative` — reservations are inert there, mirroring the unguarded
/// debit.
fn validate_reservation(
    ledger: &Ledger,
    venue: &str,
    reservation: &Option<(String, Decimal)>,
) -> Result<(), String> {
    let Some((asset, amount)) = reservation else { return Ok(()) };
    if ledger.allow_negative() {
        return Ok(());
    }
    let available = ledger.available(venue, asset);
    if *amount > available {
        return Err(format!(
            "cannot place order: requires {} {asset} on {venue}, available {} ({} reserved by resting orders)",
            amount.normalize(),
            available.normalize(),
            ledger.reserved(venue, asset).normalize(),
        ));
    }
    Ok(())
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
///
/// Placement computes the order's reservation (#124) — the exact spend its
/// fill will debit — and rejects up front when the venue's *available*
/// balance (net of other resting orders) cannot back it, instead of resting a
/// doomed order that would silently fail at fill time.
pub fn place_swap_limit(
    ledger: &Ledger,
    ctx: &dyn MarketContext,
    policy: &ResolvedPolicy,
    cfg: &SwapConfig,
) -> LimitPlacement {
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
    let reservation = swap_reservation(ctx, policy, cfg);
    if let Err(e) = validate_reservation(ledger, &cfg.chain, &reservation) {
        return LimitPlacement::Rejected(e);
    }
    LimitPlacement::Placed(PlacedLimit {
        side,
        limit,
        venue: cfg.chain.clone(),
        symbol: base.to_string(),
        reservation,
    })
}

/// Validate a perp limit order against the ledger and resolve its side.
///
/// A reduce-only limit closes an existing position (take-profit), so it requires
/// one to exist; its direction is the closing direction of that position.
///
/// An open computes its margin+fee reservation (#124) and is rejected at
/// placement when the venue's available USDC cannot back it; a reduce-only
/// close only credits, so it reserves nothing.
pub fn place_perp_limit(
    ledger: &Ledger,
    policy: &ResolvedPolicy,
    cfg: &PerpOrderConfig,
) -> LimitPlacement {
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
    let reservation = perp_reservation(policy, cfg);
    if let Err(e) = validate_reservation(ledger, &cfg.chain, &reservation) {
        return LimitPlacement::Rejected(e);
    }
    LimitPlacement::Placed(PlacedLimit {
        side,
        limit,
        venue: cfg.chain.clone(),
        symbol: cfg.symbol.clone(),
        reservation,
    })
}
