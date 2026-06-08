//! Issue #115 — non-stable (ETH) yield deposits/withdrawals mishandle units.
//!
//! `yields.rs` never reads a price or bar, so it has no access to the ETH price.
//! Two structural bugs follow:
//!   (a) Gas is computed in USD (`gas_usd`) but then `ledger.debit`-ed against the
//!       ETH balance as if it were ETH units. A 0.25 USD fee removes 0.25 ETH
//!       (~500 USD at ETH=2000). On the `all` path this same unit-mix makes
//!       `amount = balance - gas` compare ETH against USD, over-reserving and
//!       rejecting a well-funded deposit.
//!   (b) `Fill.value_usd` is set to the raw asset `amount`, not `amount * price`,
//!       so the reported notional is the ETH quantity, not its USD value.
//!
//! All four tests below record the CURRENT (incorrect) behavior — VERDICT: PRESENT.
//! The model cannot produce the USD-aware "correct" values; those are documented
//! in comments using a reference ETH price of 2000 USD (NOT fed to the model).

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

/// Issue #115 (a): gas computed in USD but debited against the ETH balance.
///
/// 1 ETH funded, deposit 0.5 ETH, gas = 0.25 USD. The model debits 0.5 ETH
/// principal then 0.25 *ETH* for the gas, leaving 0.25 ETH.
/// INCORRECT: a 0.25 USD fee at ETH=2000 USD is 0.000125 ETH, so the balance
/// should be 1 - 0.5 - 0.000125 = 0.499875 ETH. Charging 0.25 ETH (~500 USD)
/// for a 0.25 USD fee is the unit-mix bug.
#[test]
fn issue_115_deposit_gas_debited_in_eth_units_not_usd() {
    let market = FakeMarket::new().with_gas("base", "0.25");
    let mut l = ledger_with("base", "ETH", "1");
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &eth_yield("0.5"));
    let fill = out.fill().expect("executed");

    // BUG #115(a): gas (0.25 USD) was debited as 0.25 ETH. Correct: d("0.499875").
    assert_eq!(
        l.balance("base", "ETH"),
        d("0.25"),
        "issue #115: balance reflects 0.25 ETH gas; correct is 0.499875 ETH (0.25 USD @2000)"
    );
    assert_ne!(l.balance("base", "ETH"), d("0.499875"));
    // gas reported in USD but applied as ETH units.
    assert_eq!(fill.gas_usd, d("0.25"));
    // principal moved correctly (in ETH).
    assert_eq!(
        l.yield_position("aave", "ETH", "base", Some("eth")).unwrap().principal,
        d("0.5")
    );
}

/// Issue #115 (b): Fill.value_usd is set to the asset amount, not amount * price.
///
/// Depositing 0.5 ETH reports value_usd = Some(0.5). At ETH=2000 USD the notional
/// is 0.5 * 2000 = 1000 USD, so value_usd should be Some(1000). Reporting 0.5
/// understates the USD notional ~4000x.
#[test]
fn issue_115_deposit_fill_value_usd_is_asset_amount_not_usd() {
    let market = FakeMarket::new().with_gas("base", "0.25");
    let mut l = ledger_with("base", "ETH", "1");
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &eth_yield("0.5"));
    let fill = out.fill().expect("executed");

    // BUG #115(b): value_usd echoes the ETH amount. Correct: Some(d("1000")).
    assert_eq!(
        fill.value_usd,
        Some(d("0.5")),
        "issue #115: value_usd echoes ETH amount; correct is Some(1000) = 0.5 ETH * 2000 USD"
    );
    assert_ne!(fill.value_usd, Some(d("1000")));
    // value_usd is literally the asset amount.
    assert_eq!(fill.amount, Some(d("0.5")));
    assert_eq!(fill.value_usd, fill.amount);
}

/// Issue #115 (b) on the withdraw path, plus (a) gas-in-ETH again.
///
/// After depositing 0.5 ETH (gas 0.25 -> balance 0.25 ETH), withdraw 0.2 ETH:
/// credits 0.2 ETH (-> 0.45), then debits 0.25 ETH gas (-> 0.20 ETH).
/// INCORRECT: value_usd should be 0.2 * 2000 = 400 USD; balance should be
/// 0.25 + 0.2 - 0.000125 = 0.449875 ETH (0.25 USD gas in ETH), not 0.20.
#[test]
fn issue_115_withdraw_fill_value_usd_is_asset_amount_not_usd() {
    let market = FakeMarket::new().with_gas("base", "0.25");
    let policy = strict_v1();
    let mut l = ledger_with("base", "ETH", "1");
    execute_yield_deposit(&mut l, &market, &policy, &eth_yield("0.5"));
    let out = execute_yield_withdraw(&mut l, &market, &policy, &eth_yield("0.2"));
    let fill = out.fill().expect("executed");

    // BUG #115(b): value_usd echoes the ETH amount. Correct: Some(d("400")).
    assert_eq!(
        fill.value_usd,
        Some(d("0.2")),
        "issue #115: value_usd echoes ETH amount; correct is Some(400) = 0.2 ETH * 2000 USD"
    );
    assert_ne!(fill.value_usd, Some(d("400")));
    assert_eq!(fill.gas_usd, d("0.25"));
    // BUG #115(a): gas charged as 0.25 ETH. Correct: 0.449875 ETH.
    assert_eq!(
        l.balance("base", "ETH"),
        d("0.20"),
        "issue #115: gas charged as 0.25 ETH; correct is 0.449875 ETH (0.25 USD @2000)"
    );
    assert_ne!(l.balance("base", "ETH"), d("0.449875"));
}

/// Issue #115 (a) on the `all` path: `amount = balance - gas` mixes ETH and USD.
///
/// 1 ETH (~2000 USD, easily covering a 100 USD gas), gas = 100 USD, deposit "all".
/// The model computes amount = (1 - 100).max(0) = 0 (ETH compared against USD),
/// then rejects with "nothing to deposit". The ledger is left untouched.
/// INCORRECT: 100 USD gas = 0.05 ETH @2000, so it should reserve 0.05 ETH and
/// deposit ~0.95 ETH. Rejecting a 2000-USD-funded deposit because 1 ETH < 100
/// (USD) is the unit-mix bug.
#[test]
fn issue_115_deposit_all_reserves_gas_in_eth_units_zeroing_a_funded_balance() {
    let market = FakeMarket::new().with_gas("base", "100");
    let mut l = ledger_with("base", "ETH", "1");
    let out = execute_yield_deposit(&mut l, &market, &strict_v1(), &eth_yield("all"));

    // BUG #115(a): rejected because 1 ETH - 100 (USD) <= 0. Correct: deposit ~0.95 ETH.
    assert!(
        matches!(out, Execution::Rejected { .. }),
        "issue #115: 'all' deposit rejected by ETH-vs-USD unit mix; correct is to deposit ~0.95 ETH"
    );
    assert_eq!(l.balance("base", "ETH"), d("1"));
    assert!(l.yield_position("aave", "ETH", "base", Some("eth")).is_none());
}
