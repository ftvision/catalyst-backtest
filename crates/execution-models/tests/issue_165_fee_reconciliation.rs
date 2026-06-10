//! Issue #165 — recorded perp-close fees equal fees actually collected.
//!
//! The #117 settlement floor (`settlement = (gross - fee).max(0)` with
//! `gross = returned_margin + realized_pnl`) forgives part or all of the close
//! fee in cash whenever `gross < fee`. Before this fix `record_fee(fee)` still
//! recorded the *full* fee, so `fees_usd` overstated cash fees on bankrupt
//! closes and could not be reconciled against ledger deltas. Now:
//!
//!   fee_collected = min(fee, max(gross, 0))
//!
//! is what both `record_fee` and the fill's `fee_usd` report, while
//! `realized_pnl_usd` keeps the full economic PnL (a P&L statistic, not a cash
//! flow). Scenarios use entry 2000 / size_usd 1000 at 10x (margin 100,
//! size 0.5) with zero slippage and the strict_v1 5 bps fee.

use std::collections::BTreeMap;
use std::str::FromStr;

use catalyst_contracts::graph::{PerpOrderConfig, PerpSide};
use catalyst_execution_models::{execute_perp, Bar, MarketContext};
use catalyst_portfolio_ledger::Ledger;
use catalyst_simulation_policies::strict_v1;
use rust_decimal::Decimal;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

struct FakeMarket {
    bars: BTreeMap<(String, String), Bar>,
}

impl FakeMarket {
    fn with_bar(venue: &str, symbol: &str, close: &str) -> Self {
        let c = d(close);
        let mut bars = BTreeMap::new();
        bars.insert(
            (venue.into(), symbol.into()),
            Bar { open: c, high: c, low: c, close: c, volume: None },
        );
        FakeMarket { bars }
    }
}

impl MarketContext for FakeMarket {
    fn bar(&self, venue: &str, symbol: &str) -> Option<Bar> {
        self.bars.get(&(venue.into(), symbol.into())).copied()
    }
    fn gas_usd(&self, _chain: &str) -> Option<Decimal> {
        None
    }
    fn pool_reserves(&self, _venue: &str, _symbol: &str) -> Option<(Decimal, Decimal)> {
        None
    }
}

fn ledger_with_usdc(amount: &str) -> Ledger {
    let mut balances = BTreeMap::new();
    let mut a = BTreeMap::new();
    a.insert("USDC".to_string(), d(amount));
    balances.insert("hyperliquid".to_string(), a);
    Ledger::with_initial(balances, false)
}

fn perp(side: PerpSide, size_usd: &str, leverage: Option<&str>, reduce_only: bool) -> PerpOrderConfig {
    PerpOrderConfig {
        symbol: "ETH".into(),
        side,
        size_usd: size_usd.into(),
        leverage: leverage.map(|s| s.to_string()),
        chain: "hyperliquid".into(),
        order_type: "market".into(),
        reduce_only,
        limit_price: None,
        time_in_force: None,
        expire_after_bars: None,
    }
}

/// strict_v1 with zero slippage; fee stays at the profile's 5 bps.
fn policy() -> catalyst_simulation_policies::ResolvedPolicy {
    let mut p = strict_v1();
    p.slippage_bps = "0".into();
    p
}

/// Bankrupt close (`gross <= 0`): the close fee is fully forgiven in cash, so
/// `fees_usd` moves by 0 and the fill reports `fee_usd = 0`, while
/// `realized_pnl_usd` keeps the full economic loss. The ledger delta
/// reconciles exactly: initial - final == fees collected + margin lost.
#[test]
fn bankrupt_close_records_zero_fee_and_reconciles_with_cash() {
    let policy = policy();
    let mut l = ledger_with_usdc("1000");

    // Open 10x long at 2000: margin 100, size 0.5, open fee 1000 * 5bps = 0.5.
    let open_mkt = FakeMarket::with_bar("hyperliquid", "ETH", "2000");
    let opened = execute_perp(&mut l, &open_mkt, &policy, &perp(PerpSide::Long, "1000", Some("10"), false));
    assert!(opened.is_executed());
    assert_eq!(l.balance("hyperliquid", "USDC"), d("899.5"));
    assert_eq!(l.fees_usd(), d("0.5"));

    // Crash to 1700: realized = -300 * 0.5 = -150, gross = 100 - 150 = -50.
    // Bankrupt: settlement 0 AND fee (0.425 on the 850 closed notional) forgiven.
    let crash_mkt = FakeMarket::with_bar("hyperliquid", "ETH", "1700");
    let out = execute_perp(&mut l, &crash_mkt, &policy, &perp(PerpSide::Short, "1000", None, true));
    let fill = out.fill().expect("executed");

    assert_eq!(fill.fee_usd, Decimal::ZERO, "forgiven fee must not be reported as paid");
    assert_eq!(fill.realized_pnl_usd, Some(d("-150")), "economic PnL stays gross");
    assert_eq!(l.fees_usd(), d("0.5"), "fees_usd must move by exactly the cash fee (0)");
    assert!(l.perp("hyperliquid", "ETH").is_none());

    // Reconciliation: initial 1000 - final 899.5 = 100.5
    //               == fees collected 0.5 + (margin posted 100 - settlements 0).
    let final_usdc = l.balance("hyperliquid", "USDC");
    assert_eq!(final_usdc, d("899.5"));
    assert_eq!(d("1000") - final_usdc, l.fees_usd() + d("100") - Decimal::ZERO);
}

/// Partial regime (`0 < gross < fee`): only `gross` of the fee is collectable.
/// Close at 1800.4: gross = 100 - 99.8 = 0.2, fee = 900.2 * 5bps = 0.4501 =>
/// fee_collected = 0.2, settlement = 0.
#[test]
fn partially_covered_close_collects_only_gross() {
    let policy = policy();
    let mut l = ledger_with_usdc("1000");

    let open_mkt = FakeMarket::with_bar("hyperliquid", "ETH", "2000");
    execute_perp(&mut l, &open_mkt, &policy, &perp(PerpSide::Long, "1000", Some("10"), false));
    assert_eq!(l.balance("hyperliquid", "USDC"), d("899.5"));

    let mkt = FakeMarket::with_bar("hyperliquid", "ETH", "1800.4");
    let out = execute_perp(&mut l, &mkt, &policy, &perp(PerpSide::Short, "1000", None, true));
    let fill = out.fill().expect("executed");

    assert_eq!(fill.fee_usd, d("0.2"), "fee collection is capped at the gross value returned");
    assert_eq!(fill.realized_pnl_usd, Some(d("-99.8")));
    assert_eq!(l.fees_usd(), d("0.5") + d("0.2"));
    // Settlement (0.2 - 0.4501).max(0) = 0: cash unchanged by the close.
    assert_eq!(l.balance("hyperliquid", "USDC"), d("899.5"));
}

/// Healthy close (`gross >= fee`): unchanged behavior — the full fee is
/// collected and recorded; pinned so the cap never under-records normal fees.
#[test]
fn healthy_close_still_records_the_full_fee() {
    let policy = policy();
    let mut l = ledger_with_usdc("1000");

    let mkt = FakeMarket::with_bar("hyperliquid", "ETH", "2000");
    execute_perp(&mut l, &mkt, &policy, &perp(PerpSide::Long, "1000", Some("10"), false));
    // Flat close: gross = 100, fee = 0.5, settlement = 99.5.
    let out = execute_perp(&mut l, &mkt, &policy, &perp(PerpSide::Short, "1000", None, true));
    let fill = out.fill().expect("executed");

    assert_eq!(fill.fee_usd, d("0.5"));
    assert_eq!(l.fees_usd(), d("1.0")); // 0.5 open + 0.5 close
    assert_eq!(l.balance("hyperliquid", "USDC"), d("999")); // 1000 - 0.5 - 0.5
}

/// The same collected-fee rule applies on the partial-close path (the
/// `set_perp` + `credit` branch): a bankrupt half-close forgives its fee and
/// leaves the remaining half untouched.
#[test]
fn bankrupt_partial_close_forgives_fee_too() {
    let policy = policy();
    let mut l = ledger_with_usdc("1000");

    // Open 10x long, size_usd 2000 at entry 2000: margin 200, size 1, fee 1.
    let open_mkt = FakeMarket::with_bar("hyperliquid", "ETH", "2000");
    execute_perp(&mut l, &open_mkt, &policy, &perp(PerpSide::Long, "2000", Some("10"), false));
    assert_eq!(l.balance("hyperliquid", "USDC"), d("799"));
    assert_eq!(l.fees_usd(), d("1"));

    // Half-close (size_usd 1000 => 0.5 base) into the crash: returned_margin
    // 100, realized -150, gross -50 => settlement 0, fee (0.425) forgiven.
    let crash_mkt = FakeMarket::with_bar("hyperliquid", "ETH", "1700");
    let out = execute_perp(&mut l, &crash_mkt, &policy, &perp(PerpSide::Short, "1000", None, true));
    let fill = out.fill().expect("executed");

    assert_eq!(fill.fee_usd, Decimal::ZERO);
    assert_eq!(fill.realized_pnl_usd, Some(d("-150")));
    assert_eq!(l.fees_usd(), d("1"), "no cash fee was collected on the bankrupt half");
    assert_eq!(l.balance("hyperliquid", "USDC"), d("799"), "settlement floored at 0");

    let remaining = l.perp("hyperliquid", "ETH").expect("half remains");
    assert_eq!(remaining.size, d("0.5"));
    assert_eq!(remaining.margin_usd, d("100"));
}
