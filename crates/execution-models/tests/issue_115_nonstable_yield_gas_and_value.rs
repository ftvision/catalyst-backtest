//! Issue #115 (FIXED) — non-stable (ETH) yield deposit/withdraw unit handling.
//!
//! `yields.rs` now reads the asset price (`ctx.bar(chain, asset).close`, 1 for a
//! stablecoin) and:
//!   (a) converts the USD gas fee into asset units (`gas_usd / price`) before
//!       debiting it from the asset-denominated balance, and reserves gas in asset
//!       units on the `all` path (no more ETH-vs-USD comparison);
//!   (b) reports `Fill.value_usd = amount * price` (the USD notional), not the raw
//!       asset quantity.
//! A non-stable deposit/withdraw with no price this tick is rejected (can't value
//! it or convert its gas) rather than silently mixing units.
//!
//! These tests assert the corrected behavior at a reference ETH price of 2000 USD
//! (now fed to the model via the bar), plus stable-unchanged and no-price-rejects
//! edge cases.

use std::collections::BTreeMap;
use std::str::FromStr;

use catalyst_contracts::graph::YieldConfig;
use catalyst_execution_models::{
    execute_yield_deposit, execute_yield_withdraw, Bar, Execution, MarketContext,
};
use catalyst_portfolio_ledger::Ledger;
use catalyst_simulation_policies::{strict_v1, GasModel};
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    fn with_reserves(mut self, venue: &str, symbol: &str, base: &str, quote: &str) -> Self {
        self.reserves.insert((venue.into(), symbol.into()), (d(base), d(quote)));
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

fn eth_yield(amount: &str) -> YieldConfig {
    YieldConfig {
        chain: "base".into(),
        protocol: "aave".into(),
        pool: Some("eth".into()),
        asset: "ETH".into(),
        amount: amount.into(),
    }
}

// Sanity: strict_v1 uses HistoricalFeeHistory so gas is read from ctx.gas_usd.
const _: fn() = || {
    let _ = GasModel::HistoricalFeeHistory;
};

/// Issue #115 (a) — FIXED: gas (USD) is converted to asset units before it's
/// debited from the ETH balance.
///
/// 1 ETH funded (priced 2000), deposit 0.5 ETH, gas = 0.25 USD = 0.000125 ETH.
/// Balance = 1 - 0.5 (principal) - 0.000125 (gas) = 0.499875 ETH.
#[test]
fn issue_115_deposit_gas_converted_to_asset_units() {
    let market = FakeMarket::new().with_bar("base", "ETH", "2000").with_gas("base", "0.25");
    let mut l = ledger_with("base", "ETH", "1");
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &eth_yield("0.5"));
    let fill = out.fill().expect("executed");

    assert_eq!(
        l.balance("base", "ETH"),
        d("0.499875"),
        "issue #115(a) FIXED: 0.25 USD gas @2000 = 0.000125 ETH; balance 1 - 0.5 - 0.000125"
    );
    assert_eq!(fill.gas_usd, d("0.25")); // gas still reported in USD
    assert_eq!(
        l.yield_position("aave", "ETH", "base", Some("eth")).unwrap().principal,
        d("0.5")
    );
}

/// Issue #115 (b) — FIXED: Fill.value_usd is amount * price, not the raw amount.
///
/// Depositing 0.5 ETH @2000 reports value_usd = Some(1000).
#[test]
fn issue_115_deposit_value_usd_is_amount_times_price() {
    let market = FakeMarket::new().with_bar("base", "ETH", "2000").with_gas("base", "0.25");
    let mut l = ledger_with("base", "ETH", "1");
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &eth_yield("0.5"));
    let fill = out.fill().expect("executed");

    assert_eq!(
        fill.value_usd,
        Some(d("1000")),
        "issue #115(b) FIXED: value_usd = 0.5 ETH * 2000 USD = 1000"
    );
    assert_eq!(fill.amount, Some(d("0.5"))); // amount stays in asset units
}

/// Issue #115 (a)+(b) — FIXED on the withdraw path.
///
/// Deposit 0.5 ETH (gas 0.000125 ETH -> balance 0.499875), then withdraw 0.2 ETH:
/// credit 0.2 (-> 0.699875), debit gas 0.000125 (-> 0.69975). value_usd = 0.2*2000 = 400.
#[test]
fn issue_115_withdraw_value_usd_is_amount_times_price() {
    let market = FakeMarket::new().with_bar("base", "ETH", "2000").with_gas("base", "0.25");
    let policy = strict_v1();
    let mut l = ledger_with("base", "ETH", "1");
    execute_yield_deposit(&mut l, &market, &policy, &eth_yield("0.5"));
    let out = execute_yield_withdraw(&mut l, &market, &policy, &eth_yield("0.2"));
    let fill = out.fill().expect("executed");

    assert_eq!(
        fill.value_usd,
        Some(d("400")),
        "issue #115(b) FIXED: value_usd = 0.2 ETH * 2000 USD = 400"
    );
    assert_eq!(fill.gas_usd, d("0.25"));
    assert_eq!(
        l.balance("base", "ETH"),
        d("0.69975"),
        "issue #115(a) FIXED: 0.499875 + 0.2 - 0.000125 = 0.69975 ETH"
    );
}

/// Issue #115 (a) — FIXED on the `all` path: gas is reserved in asset units.
///
/// 1 ETH @2000, gas = 100 USD = 0.05 ETH, deposit "all": reserve 0.05 ETH and
/// deposit 0.95 ETH (no longer wrongly rejected by an ETH-vs-USD comparison).
#[test]
fn issue_115_deposit_all_reserves_gas_in_asset_units() {
    let market = FakeMarket::new().with_bar("base", "ETH", "2000").with_gas("base", "100");
    let mut l = ledger_with("base", "ETH", "1");
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &eth_yield("all"));
    let fill = out.fill().expect("executed");

    assert_eq!(
        l.yield_position("aave", "ETH", "base", Some("eth")).unwrap().principal,
        d("0.95"),
        "issue #115(a) FIXED: 100 USD gas @2000 = 0.05 ETH reserved; deposit 0.95 ETH"
    );
    assert_eq!(l.balance("base", "ETH"), d("0")); // 1 - 0.95 deposited - 0.05 gas
    assert_eq!(fill.value_usd, Some(d("1900"))); // 0.95 * 2000
}

/// Edge: a STABLE-asset (USDC) yield deposit is unchanged — price is 1, so gas in
/// asset units == gas in USD and value_usd == amount. (Guards against regressing
/// the original USDC-only path.)
#[test]
fn issue_115_stable_yield_deposit_unchanged() {
    let market = FakeMarket::new().with_gas("base", "0.25"); // no bar needed for a stable
    let mut l = ledger_with("base", "USDC", "1000");
    let cfg = YieldConfig {
        chain: "base".into(),
        protocol: "aave".into(),
        pool: Some("usdc".into()),
        asset: "USDC".into(),
        amount: "500".into(),
    };
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &cfg);
    let fill = out.fill().expect("executed");
    assert_eq!(l.balance("base", "USDC"), d("499.75")); // 1000 - 500 - 0.25 gas
    assert_eq!(fill.value_usd, Some(d("500"))); // 500 USDC * 1
}

/// Edge: a non-stable yield deposit with NO price this tick can't be valued or its
/// gas converted, so it is rejected with the ledger untouched (rather than silently
/// charging gas in mixed units).
#[test]
fn issue_115_nonstable_deposit_without_price_is_rejected() {
    let market = FakeMarket::new().with_gas("base", "0.25"); // no ETH bar
    let mut l = ledger_with("base", "ETH", "1");
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &eth_yield("0.5"));
    assert!(matches!(out, Execution::Rejected { .. }), "issue #115: non-stable with no price rejects");
    assert_eq!(l.balance("base", "ETH"), d("1"));
    assert!(l.yield_position("aave", "ETH", "base", Some("eth")).is_none());
}
