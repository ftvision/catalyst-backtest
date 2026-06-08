//! Issue #117 — confirms #117 fixed by PR #133 (regression guard).
//!
//! PR #133 fixed both sub-bugs; these tests assert the CORRECT post-fix
//! behavior and guard against regression:
//!
//! - `issue_117_leveraged_long_loss_capped_at_margin` — perp.rs full-close now
//!   floors the settlement at zero, so a leveraged loss can never exceed the
//!   posted margin (USDC does not dip below the post-open margin floor).
//! - `issue_117_dust_sell_rejected` — swap.rs sell path now rejects when
//!   gas+fee swallow the proceeds (net <= 0) instead of crediting a negative
//!   net as phantom destination-asset debt; the ledger is left untouched.

use std::collections::BTreeMap;
use std::str::FromStr;

use catalyst_contracts::graph::{PerpOrderConfig, PerpSide, SwapConfig};
use catalyst_execution_models::{execute_perp, execute_swap, Bar, Execution, MarketContext};
use catalyst_portfolio_ledger::Ledger;
use catalyst_simulation_policies::strict_v1;
use rust_decimal::Decimal;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

struct FakeMarket {
    bars: BTreeMap<(String, String), Bar>,
    gas: BTreeMap<String, Decimal>,
    reserves: BTreeMap<(String, String), (Decimal, Decimal)>,
}

impl FakeMarket {
    fn new() -> Self {
        FakeMarket { bars: BTreeMap::new(), gas: BTreeMap::new(), reserves: BTreeMap::new() }
    }
    fn with_bar(mut self, venue: &str, symbol: &str, close: &str) -> Self {
        let c = d(close);
        self.bars.insert(
            (venue.into(), symbol.into()),
            Bar { open: c, high: c * d("1.02"), low: c * d("0.98"), close: c, volume: None },
        );
        self
    }
    fn with_gas(mut self, chain: &str, usd: &str) -> Self {
        self.gas.insert(chain.into(), d(usd));
        self
    }
}

impl MarketContext for FakeMarket {
    fn bar(&self, venue: &str, symbol: &str) -> Option<Bar> {
        self.bars.get(&(venue.into(), symbol.into())).copied()
    }
    fn gas_usd(&self, chain: &str) -> Option<Decimal> {
        self.gas.get(chain).copied()
    }
    fn pool_reserves(&self, venue: &str, symbol: &str) -> Option<(Decimal, Decimal)> {
        self.reserves.get(&(venue.into(), symbol.into())).copied()
    }
}

fn ledger_with(venue: &str, asset: &str, amount: &str) -> Ledger {
    let mut balances = BTreeMap::new();
    let mut a = BTreeMap::new();
    a.insert(asset.to_string(), d(amount));
    balances.insert(venue.to_string(), a);
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

fn swap(from: &str, to: &str, amount: &str, chain: &str) -> SwapConfig {
    SwapConfig {
        from_asset: from.into(),
        to_asset: to.into(),
        amount: amount.into(),
        chain: chain.into(),
        order_type: "market".into(),
        limit_price: None,
        time_in_force: None,
        expire_after_bars: None,
    }
}

/// Confirms #117 fixed by PR #133 (regression guard): a leveraged long that goes
/// deep underwater caps its loss at the posted margin — the full close floors the
/// settlement at zero instead of clawing back USDC the trader never posted.
///
/// Open 10x long, size_usd=1000 at entry=100: margin=100, size=10. USDC: 1000->900.
/// Close at price=85: realized_pnl = (85-100)*10 = -150, returned_margin=100,
/// fee=0 -> raw settlement = 100 + (-150) - 0 = -50. perp.rs floors this at zero
/// (`settlement = (..).max(0)`), so 0 is credited and USDC stays 900 — the trader
/// loses exactly the 100 posted margin, never more.
#[test]
fn issue_117_leveraged_long_loss_capped_at_margin() {
    let mut policy = strict_v1();
    policy.slippage_bps = "0".into();
    policy.fee_bps = "0".into();

    let mut l = ledger_with("hyperliquid", "USDC", "1000");

    // Open 10x long at price 100.
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "100");
    let opened = execute_perp(&mut l, &market, &policy, &perp(PerpSide::Long, "1000", Some("10"), false));
    assert!(opened.is_executed());
    assert_eq!(l.balance("hyperliquid", "USDC"), d("900")); // 1000 - 100 margin

    // Price drops to 85; reduce-only close of the full notional.
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "85");
    let out = execute_perp(&mut l, &market, &policy, &perp(PerpSide::Long, "1000", None, true));
    assert!(out.is_executed());
    assert_eq!(out.fill().unwrap().realized_pnl_usd, Some(d("-150")));
    assert!(l.perp("hyperliquid", "ETH").is_none());

    let usdc = l.balance("hyperliquid", "USDC");
    // Fixed by #133: raw settlement -50 is floored at 0, so nothing is credited
    // back and USDC stays at the 900 post-open margin floor.
    assert_eq!(
        usdc,
        d("900"),
        "loss must cap at the 100 posted margin (settlement floored at 0); usdc={usdc}"
    );
    // The loss is exactly the posted margin: USDC never dips below the 900 floor.
    assert!(
        usdc >= d("900"),
        "trader lost more than the 100 posted margin: usdc={usdc} fell below the 900 floor"
    );
}

/// Confirms #117 fixed by PR #133 (regression guard): a dust sell whose gas+fee
/// exceed the proceeds is REJECTED, not credited as a negative net (which would
/// mint phantom destination-asset debt), and the ledger is left untouched.
///
/// Sell 0.0002 ETH at price=2000 on base (EVM, gas applies): proceeds=0.4,
/// fee = 0.4 * 5bps = 0.0002, gas=0.5. swap.rs computes
/// net = 0.4 - 0.0002 - 0.5 = -0.1002; since net <= 0 the swap is rejected
/// before any debit/credit, so USDC stays 0 and ETH stays 0.0002.
#[test]
fn issue_117_dust_sell_rejected() {
    let mut policy = strict_v1();
    policy.slippage_bps = "0".into(); // fee_bps left at default 5

    let market = FakeMarket::new().with_bar("base", "ETH", "2000").with_gas("base", "0.5");

    // Seed both ETH and USDC on the base venue, strict policy.
    let mut balances = BTreeMap::new();
    let mut a = BTreeMap::new();
    a.insert("ETH".to_string(), d("0.0002"));
    a.insert("USDC".to_string(), d("0"));
    balances.insert("base".to_string(), a);
    let mut l = Ledger::with_initial(balances, false);

    let out = execute_swap(&mut l, &market, &policy, &swap("ETH", "USDC", "0.0002", "base"));

    // Fixed by #133: the dust sell is rejected (net <= 0), not executed.
    assert!(
        matches!(out, Execution::Rejected { .. }),
        "dust sell below cost (net <= 0) must be Rejected, was {out:?}"
    );
    // Ledger untouched: no phantom USDC debt minted, ETH not debited.
    assert_eq!(
        l.balance("base", "USDC"),
        Decimal::ZERO,
        "rejected swap must not mint phantom USDC debt"
    );
    assert_eq!(
        l.balance("base", "ETH"),
        d("0.0002"),
        "rejected swap must leave the source ETH untouched"
    );
}
