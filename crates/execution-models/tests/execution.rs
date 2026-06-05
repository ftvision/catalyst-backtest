//! Tests for the execution models.

use std::collections::BTreeMap;
use std::str::FromStr;

use catalyst_contracts::graph::{PerpOrderConfig, PerpSide, SwapConfig, YieldConfig};
use catalyst_execution_models::{
    execute_perp, execute_swap, execute_yield_deposit, execute_yield_withdraw, Bar, Execution,
    MarketContext,
};
use catalyst_portfolio_ledger::Ledger;
use catalyst_simulation_policies::{strict_v1, ResolvedPolicy};
use rust_decimal::Decimal;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

struct FakeMarket {
    bars: BTreeMap<(String, String), Bar>,
    gas: BTreeMap<String, Decimal>,
}

impl FakeMarket {
    fn new() -> Self {
        FakeMarket { bars: BTreeMap::new(), gas: BTreeMap::new() }
    }
    fn with_bar(mut self, venue: &str, symbol: &str, close: &str) -> Self {
        let c = d(close);
        self.bars.insert(
            (venue.into(), symbol.into()),
            Bar { open: c, high: c * d("1.02"), low: c * d("0.98"), close: c },
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
}

fn ledger_with(venue: &str, asset: &str, amount: &str) -> Ledger {
    let mut balances = BTreeMap::new();
    let mut a = BTreeMap::new();
    a.insert(asset.to_string(), d(amount));
    balances.insert(venue.to_string(), a);
    Ledger::with_initial(balances, false)
}

fn swap(from: &str, to: &str, amount: &str, chain: &str) -> SwapConfig {
    SwapConfig {
        from_asset: from.into(),
        to_asset: to.into(),
        amount: amount.into(),
        chain: chain.into(),
    }
}

// --- Swaps: slippage, fees, gas ---

#[test]
fn evm_buy_applies_slippage_fee_and_gas() {
    let market = FakeMarket::new().with_bar("base", "ETH", "2000").with_gas("base", "0.02");
    let mut l = ledger_with("base", "USDC", "1000");
    let out = execute_swap(&mut l, &market, &strict_v1(), &swap("USDC", "ETH", "100", "base"));
    let fill = out.fill().expect("executed");
    // close=2000, +10bps slippage => 2002 fill
    assert_eq!(fill.price, Some(d("2002")));
    assert_eq!(fill.fee_usd, d("0.05")); // 100 * 5bps
    assert_eq!(fill.gas_usd, d("0.02"));
    // 100 USDC notional + 0.05 fee + 0.02 gas leaves the account
    assert_eq!(l.balance("base", "USDC"), d("899.93"));
    assert_eq!(fill.amount, Some(d("100") / d("2002")));
}

#[test]
fn sell_applies_adverse_slippage() {
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "2000");
    let mut l = ledger_with("hyperliquid", "ETH", "1");
    let out = execute_swap(&mut l, &market, &strict_v1(), &swap("ETH", "USDC", "0.5", "hyperliquid"));
    let fill = out.fill().expect("executed");
    // sells fill 10bps lower => 1998
    assert_eq!(fill.price, Some(d("1998")));
    // hyperliquid spot has no gas
    assert_eq!(fill.gas_usd, Decimal::ZERO);
}

#[test]
fn buy_with_insufficient_balance_is_rejected_and_ledger_unchanged() {
    let market = FakeMarket::new().with_bar("base", "ETH", "2000");
    let mut l = ledger_with("base", "USDC", "50");
    let out = execute_swap(&mut l, &market, &strict_v1(), &swap("USDC", "ETH", "100", "base"));
    assert!(matches!(out, Execution::Rejected { .. }));
    assert_eq!(l.balance("base", "USDC"), d("50"));
    assert_eq!(l.balance("base", "ETH"), Decimal::ZERO);
}

#[test]
fn sell_more_than_held_is_rejected() {
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "2000");
    let mut l = ledger_with("hyperliquid", "ETH", "0.03");
    let out =
        execute_swap(&mut l, &market, &strict_v1(), &swap("ETH", "USDC", "0.04", "hyperliquid"));
    assert!(matches!(out, Execution::Rejected { .. }));
    assert_eq!(l.balance("hyperliquid", "ETH"), d("0.03"));
}

#[test]
fn swap_without_a_stable_side_is_rejected() {
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "2000");
    let mut l = ledger_with("hyperliquid", "BTC", "1");
    let out = execute_swap(&mut l, &market, &strict_v1(), &swap("BTC", "ETH", "1", "hyperliquid"));
    assert!(matches!(out, Execution::Rejected { .. }));
}

// --- Perps: open, add, reduce-only close ---

fn perp(side: PerpSide, size_usd: &str, leverage: Option<&str>, reduce_only: bool) -> PerpOrderConfig {
    PerpOrderConfig {
        symbol: "ETH".into(),
        side,
        size_usd: size_usd.into(),
        leverage: leverage.map(|s| s.to_string()),
        chain: "hyperliquid".into(),
        order_type: "market".into(),
        reduce_only,
    }
}

#[test]
fn open_perp_debits_margin_and_fee() {
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "2000");
    let mut l = ledger_with("hyperliquid", "USDC", "1000");
    let out = execute_perp(&mut l, &market, &strict_v1(), &perp(PerpSide::Long, "500", Some("5"), false));
    let fill = out.fill().expect("executed");
    assert_eq!(fill.kind, "perp_open");
    assert_eq!(fill.fee_usd, d("0.25")); // 500 * 5bps
    // margin 100 (500/5) + 0.25 fee
    assert_eq!(l.balance("hyperliquid", "USDC"), d("899.75"));
    let pos = l.perp("hyperliquid", "ETH").unwrap();
    assert_eq!(pos.entry_price, d("2002")); // long buys at +10bps
}

#[test]
fn reduce_only_without_position_is_rejected() {
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "2000");
    let mut l = ledger_with("hyperliquid", "USDC", "1000");
    let out = execute_perp(&mut l, &market, &strict_v1(), &perp(PerpSide::Short, "500", None, true));
    assert!(matches!(out, Execution::Rejected { .. }));
}

#[test]
fn open_then_full_close_removes_position_and_settles() {
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "2000");
    let mut l = ledger_with("hyperliquid", "USDC", "1000");
    let policy = strict_v1();
    execute_perp(&mut l, &market, &policy, &perp(PerpSide::Long, "500", Some("5"), false));
    // close at the same bar with the original notional -> closes the whole size
    let out = execute_perp(&mut l, &market, &policy, &perp(PerpSide::Short, "500", None, true));
    let fill = out.fill().expect("executed");
    assert_eq!(fill.kind, "perp_close");
    assert!(l.perp("hyperliquid", "ETH").is_none());
    // round trip loses a little to slippage + fees
    let usdc = l.balance("hyperliquid", "USDC");
    assert!(usdc > d("997") && usdc < d("1000"), "usdc was {usdc}");
}

#[test]
fn adding_same_side_increases_size_and_blends_entry() {
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "2000");
    let mut l = ledger_with("hyperliquid", "USDC", "1000");
    let policy = strict_v1();
    execute_perp(&mut l, &market, &policy, &perp(PerpSide::Long, "500", Some("5"), false));
    execute_perp(&mut l, &market, &policy, &perp(PerpSide::Long, "500", Some("5"), false));
    let pos = l.perp("hyperliquid", "ETH").unwrap();
    // two fills accumulate (each 500/2002), entry blends to the same 2002
    assert_eq!(pos.size, d("500") / d("2002") + d("500") / d("2002"));
    assert_eq!(pos.entry_price, d("2002"));
    assert_eq!(pos.margin_usd, d("200"));
}

// --- Yields ---

fn yield_cfg(amount: &str) -> YieldConfig {
    YieldConfig {
        chain: "base".into(),
        protocol: "aave".into(),
        pool: Some("usdc".into()),
        asset: "USDC".into(),
        amount: amount.into(),
    }
}

#[test]
fn yield_deposit_moves_principal_and_charges_gas() {
    let market = FakeMarket::new().with_gas("base", "0.02");
    let mut l = ledger_with("base", "USDC", "300");
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &yield_cfg("250"));
    let fill = out.fill().expect("executed");
    assert_eq!(fill.kind, "yield_deposit");
    assert_eq!(l.balance("base", "USDC"), d("49.98")); // 300 - 250 - 0.02 gas
    let pos = l.yield_position("aave", "USDC", "base", Some("usdc")).unwrap();
    assert_eq!(pos.principal, d("250"));
}

#[test]
fn yield_deposit_insufficient_is_rejected() {
    let market = FakeMarket::new().with_gas("base", "0.02");
    let mut l = ledger_with("base", "USDC", "50");
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &yield_cfg("250"));
    assert!(matches!(out, Execution::Rejected { .. }));
    assert_eq!(l.balance("base", "USDC"), d("50"));
}

#[test]
fn yield_withdraw_partial_returns_funds() {
    let market = FakeMarket::new().with_gas("base", "0.02");
    let mut l = ledger_with("base", "USDC", "300");
    let policy = strict_v1();
    execute_yield_deposit(&mut l, &market, &policy, &yield_cfg("250"));
    let out = execute_yield_withdraw(&mut l, &market, &policy, &yield_cfg("100"));
    assert!(out.is_executed());
    assert_eq!(l.balance("base", "USDC"), d("149.96")); // 49.98 + 100 - 0.02 gas
    let pos = l.yield_position("aave", "USDC", "base", Some("usdc")).unwrap();
    assert_eq!(pos.principal, d("150"));
}

#[test]
fn yield_withdraw_all_empties_position() {
    let market = FakeMarket::new(); // no gas data, policy fallback applies
    let mut policy = strict_v1();
    policy.gas_model = catalyst_simulation_policies::GasModel::None; // isolate principal accounting
    let mut l = ledger_with("base", "USDC", "250");
    execute_yield_deposit(&mut l, &market, &policy, &yield_cfg("250"));
    let out = execute_yield_withdraw(&mut l, &market, &policy, &yield_cfg("all"));
    assert!(out.is_executed());
    assert_eq!(l.balance("base", "USDC"), d("250"));
    assert!(l.yield_position("aave", "USDC", "base", Some("usdc")).is_none());
}

// --- Policy plumbing: a custom resolved policy flows through ---

#[test]
fn zero_slippage_zero_fee_policy_fills_at_close() {
    let market = FakeMarket::new().with_bar("base", "ETH", "2000").with_gas("base", "0");
    let mut p: ResolvedPolicy = strict_v1();
    p.slippage_bps = "0".into();
    p.fee_bps = "0".into();
    let mut l = ledger_with("base", "USDC", "1000");
    let out = execute_swap(&mut l, &market, &p, &swap("USDC", "ETH", "100", "base"));
    let fill = out.fill().unwrap();
    assert_eq!(fill.price, Some(d("2000")));
    assert_eq!(fill.fee_usd, Decimal::ZERO);
    assert_eq!(l.balance("base", "ETH"), d("0.05")); // 100 / 2000
}
