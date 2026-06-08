//! Tests for the execution models.

use std::collections::BTreeMap;
use std::str::FromStr;

use catalyst_contracts::graph::{PerpOrderConfig, PerpSide, SwapConfig, YieldConfig};
use catalyst_execution_models::{
    execute_perp, execute_swap, execute_yield_deposit, execute_yield_withdraw,
    limit_fill_price, place_perp_limit, place_swap_limit, Bar, Execution, LimitPlacement, LimitSide,
    MarketContext,
};
use catalyst_portfolio_ledger::Ledger;
use catalyst_simulation_policies::{strict_v1, ResolvedPolicy, SlippageModel};
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
    fn with_bar_volume(mut self, venue: &str, symbol: &str, close: &str, volume: &str) -> Self {
        let c = d(close);
        self.bars.insert(
            (venue.into(), symbol.into()),
            Bar { open: c, high: c * d("1.02"), low: c * d("0.98"), close: c, volume: Some(d(volume)) },
        );
        self
    }
    fn with_gas(mut self, chain: &str, usd: &str) -> Self {
        self.gas.insert(chain.into(), d(usd));
        self
    }
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
        limit_price: None,
        time_in_force: None,
        expire_after_bars: None,
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

// --- Limit orders: touch logic + placement validation ---

fn bar(open: &str, high: &str, low: &str, close: &str) -> Bar {
    Bar { open: d(open), high: d(high), low: d(low), close: d(close), volume: None }
}

#[test]
fn buy_limit_touches_when_low_reaches_it() {
    let b = bar("1980", "1985", "1850", "1900");
    // low 1850 <= 1900 -> fills, at the limit (open 1980 is above it)
    assert_eq!(limit_fill_price(&b, LimitSide::Buy, d("1900")), Some(d("1900")));
    // a limit below the whole bar's low is not reached
    assert_eq!(limit_fill_price(&b, LimitSide::Buy, d("1840")), None);
}

#[test]
fn buy_limit_gap_through_fills_at_open() {
    let b = bar("1850", "1860", "1820", "1840");
    // opens below the 1900 limit -> the better open price, not the limit
    assert_eq!(limit_fill_price(&b, LimitSide::Buy, d("1900")), Some(d("1850")));
}

#[test]
fn sell_limit_touches_when_high_reaches_it() {
    let b = bar("2100", "2300", "2090", "2250");
    assert_eq!(limit_fill_price(&b, LimitSide::Sell, d("2200")), Some(d("2200")));
    // gap up: opens above the limit -> fills at the better open
    let gap = bar("2250", "2300", "2240", "2280");
    assert_eq!(limit_fill_price(&gap, LimitSide::Sell, d("2200")), Some(d("2250")));
    // limit above the bar's high is not reached
    assert_eq!(limit_fill_price(&b, LimitSide::Sell, d("2400")), None);
}

fn limit_swap(from: &str, to: &str, limit: Option<&str>) -> SwapConfig {
    SwapConfig {
        from_asset: from.into(),
        to_asset: to.into(),
        amount: "100".into(),
        chain: "base".into(),
        order_type: "limit".into(),
        limit_price: limit.map(|s| s.to_string()),
        time_in_force: None,
        expire_after_bars: None,
    }
}

#[test]
fn place_swap_limit_resolves_side_and_rejects_bad_input() {
    match place_swap_limit(&limit_swap("USDC", "ETH", Some("1900"))) {
        LimitPlacement::Placed(p) => {
            assert_eq!(p.side, LimitSide::Buy);
            assert_eq!(p.symbol, "ETH");
            assert_eq!(p.limit, d("1900"));
        }
        LimitPlacement::Rejected(e) => panic!("expected placed: {e}"),
    }
    // selling the base resolves to a sell
    assert!(matches!(
        place_swap_limit(&limit_swap("ETH", "USDC", Some("2100"))),
        LimitPlacement::Placed(p) if p.side == LimitSide::Sell
    ));
    // missing limit_price is rejected
    assert!(matches!(place_swap_limit(&limit_swap("USDC", "ETH", None)), LimitPlacement::Rejected(_)));
    // no stable side is rejected
    assert!(matches!(
        place_swap_limit(&limit_swap("BTC", "ETH", Some("1"))),
        LimitPlacement::Rejected(_)
    ));
}

fn limit_perp(side: PerpSide, reduce_only: bool, limit: Option<&str>) -> PerpOrderConfig {
    PerpOrderConfig {
        symbol: "ETH".into(),
        side,
        size_usd: "500".into(),
        leverage: Some("2".into()),
        chain: "hyperliquid".into(),
        order_type: "limit".into(),
        reduce_only,
        limit_price: limit.map(|s| s.to_string()),
        time_in_force: None,
        expire_after_bars: None,
    }
}

#[test]
fn place_perp_limit_open_long_is_a_buy() {
    let l = ledger_with("hyperliquid", "USDC", "1000");
    match place_perp_limit(&l, &limit_perp(PerpSide::Long, false, Some("1900"))) {
        LimitPlacement::Placed(p) => assert_eq!(p.side, LimitSide::Buy),
        LimitPlacement::Rejected(e) => panic!("expected placed: {e}"),
    }
}

#[test]
fn place_reduce_only_limit_requires_a_position_and_closes_it() {
    // no position -> rejected
    let empty = ledger_with("hyperliquid", "USDC", "1000");
    assert!(matches!(
        place_perp_limit(&empty, &limit_perp(PerpSide::Short, true, Some("2200"))),
        LimitPlacement::Rejected(_)
    ));

    // open a long via the market path, then a reduce-only limit closes it (a sell)
    let mut l = ledger_with("hyperliquid", "USDC", "1000");
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "2000");
    let opened = execute_perp(&mut l, &market, &strict_v1(), &perp(PerpSide::Long, "500", Some("2"), false));
    assert!(opened.is_executed());
    assert!(matches!(
        place_perp_limit(&l, &limit_perp(PerpSide::Short, true, Some("2200"))),
        LimitPlacement::Placed(p) if p.side == LimitSide::Sell
    ));
}

// --- depth-aware slippage / AMM price impact (#40) ---

fn amm_policy() -> ResolvedPolicy {
    ResolvedPolicy { slippage_model: SlippageModel::AmmPriceImpact, ..strict_v1() }
}

#[test]
fn amm_buy_applies_price_impact_from_reserves() {
    // small pool: 100 ETH / 200_000 USDC (mid ~2000). Buying 2000 USDC moves price:
    // avg price = (rq + amount)/rb = (200000 + 2000)/100 = 2020, not the 2002 a
    // fixed-10bps fill on a 2000 close would give.
    let market = FakeMarket::new()
        .with_bar("base", "ETH", "2000")
        .with_reserves("base", "ETH", "100", "200000");
    let mut l = ledger_with("base", "USDC", "5000");
    let out = execute_swap(&mut l, &market, &amm_policy(), &swap("USDC", "ETH", "2000", "base"));
    let fill = out.fill().expect("executed");
    assert_eq!(fill.price, Some(d("2020")));
    assert_eq!(fill.amount, Some(d("2000") / d("2020"))); // fewer tokens than fixed

    // same trade under fixed_bps fills cheaper (no depth impact) -> more tokens
    let fixed = execute_swap(&mut l.clone(), &market, &strict_v1(), &swap("USDC", "ETH", "2000", "base"));
    assert_eq!(fixed.fill().unwrap().price, Some(d("2002")));
}

#[test]
fn amm_sell_applies_price_impact_from_reserves() {
    let market = FakeMarket::new()
        .with_bar("base", "ETH", "2000")
        .with_reserves("base", "ETH", "100", "200000");
    let mut l = ledger_with("base", "ETH", "10");
    let out = execute_swap(&mut l, &market, &amm_policy(), &swap("ETH", "USDC", "1", "base"));
    // selling 1 ETH: avg price = rq/(rb+amount) = 200000/101 ≈ 1980.198 (impact down)
    let price: f64 = out.fill().unwrap().price.unwrap().to_string().parse().unwrap();
    assert!((1980.0..1981.0).contains(&price), "price was {price}");
}

#[test]
fn amm_falls_back_to_fixed_bps_without_reserves() {
    // amm policy but no pool reserves present -> falls back to the configured bps
    // (a real cost), not zero. strict_v1 default is 10 bps -> 2002 on a 2000 close.
    let market = FakeMarket::new().with_bar("base", "ETH", "2000");
    let mut l = ledger_with("base", "USDC", "5000");
    let out = execute_swap(&mut l, &market, &amm_policy(), &swap("USDC", "ETH", "100", "base"));
    assert_eq!(out.fill().unwrap().price, Some(d("2002")));
}

// --- slippage models: one trade, four models, distinct fills (executable doc) ---
// Companion to docs/logic/slippage-models.md. The same Base-DEX buy (2000 USDC of
// ETH into a 100 ETH / 200k USDC pool, mid 2000) fills differently per model.

fn slip(model: SlippageModel, bps: &str) -> ResolvedPolicy {
    ResolvedPolicy { slippage_model: model, slippage_bps: bps.into(), ..strict_v1() }
}

#[test]
fn slippage_models_produce_distinct_swap_fills() {
    let market = FakeMarket::new()
        .with_bar("base", "ETH", "2000")
        .with_reserves("base", "ETH", "100", "200000"); // mid 2000
    let trade = swap("USDC", "ETH", "2000", "base");
    let fill = |p: &ResolvedPolicy| {
        execute_swap(&mut ledger_with("base", "USDC", "5000"), &market, p, &trade)
            .fill()
            .unwrap()
            .price
            .unwrap()
    };

    let none = fill(&slip(SlippageModel::None, "10"));
    let volume = fill(&slip(SlippageModel::VolumeBased, "10"));
    let fixed = fill(&slip(SlippageModel::FixedBps, "10"));
    let amm = fill(&slip(SlippageModel::AmmPriceImpact, "10"));

    // none = idealized mid fill.
    assert_eq!(none, d("2000"));
    // fixed_bps: flat +10bps adverse, size-independent.
    assert_eq!(fixed, d("2002"));
    // volume_based with NO bar volume falls back to fixed_bps (never silent zero).
    assert_eq!(volume, d("2002"));
    // amm_price_impact: constant-product, size-dependent -> (200000+2000)/100 = 2020.
    assert_eq!(amm, d("2020"));

    // Adverse ordering: idealized < flat-bps (= volume fallback) < depth-aware.
    assert!(none < fixed && fixed < amm, "{none} < {fixed} < {amm}");
}

// --- volume_based (square-root law) — participation-scaled slippage (#137) ---

#[test]
fn volume_based_charges_more_for_a_larger_share_of_bar_volume() {
    // Same bar (close 2000, volume 1000 ETH), same base bps; a trade that's a
    // bigger fraction of the bar's volume pays progressively more (sqrt law).
    let market = FakeMarket::new().with_bar_volume("base", "ETH", "2000", "1000");
    let buy = |usd: &str| {
        execute_swap(
            &mut ledger_with("base", "USDC", "10000000"),
            &market,
            &slip(SlippageModel::VolumeBased, "10"),
            &swap("USDC", "ETH", usd, "base"),
        )
        .fill()
        .unwrap()
        .price
        .unwrap()
    };

    // base ETH ~= usd/2000. p = base/1000. eff_bps = 10 + 50*sqrt(p).
    // $20k -> base 10, p=0.01 -> 10 + 50*0.1 = 15 bps -> 2003.0
    // $500k -> base 250, p=0.25 -> 10 + 50*0.5 = 35 bps -> 2007.0
    // $2M -> base 1000, p=1.0 -> 10 + 50*1 = 60 bps -> 2012.0
    let small = buy("20000");
    let mid = buy("500000");
    let large = buy("2000000");

    assert!(small < mid && mid < large, "monotonic in size: {small} {mid} {large}");
    // sub-linear: 100x the size (p 0.01 -> 1.0) is only ~12x the extra impact, not 100x.
    assert!((small - d("2003")).abs() < d("0.5"), "small ~2003, was {small}");
    assert!((large - d("2012")).abs() < d("0.5"), "large ~2012, was {large}");
}

#[test]
fn volume_based_falls_back_to_fixed_bps_when_bar_has_no_volume() {
    // Dune-derived candles carry no volume -> fall back to the configured bps,
    // never silently zero. Same bar without volume => 2002, like fixed_bps.
    let market = FakeMarket::new().with_bar("base", "ETH", "2000");
    let out = execute_swap(
        &mut ledger_with("base", "USDC", "5000"),
        &market,
        &slip(SlippageModel::VolumeBased, "10"),
        &swap("USDC", "ETH", "100", "base"),
    );
    assert_eq!(out.fill().unwrap().price, Some(d("2002")));
}

#[test]
fn amm_price_impact_falls_back_to_fixed_bps_for_perps() {
    // amm_price_impact's depth model is swap-only (it reads pool reserves). A perp
    // under it falls back to the configured bps (a real cost), NOT zero slippage —
    // so it matches fixed_bps here, both entering at 2002 (#136).
    let market = FakeMarket::new().with_bar("hyperliquid", "ETH", "2000");

    let mut l_fixed = ledger_with("hyperliquid", "USDC", "1000");
    execute_perp(&mut l_fixed, &market, &slip(SlippageModel::FixedBps, "10"),
                 &perp(PerpSide::Long, "500", Some("5"), false));
    assert_eq!(l_fixed.perp("hyperliquid", "ETH").unwrap().entry_price, d("2002"));

    let mut l_amm = ledger_with("hyperliquid", "USDC", "1000");
    execute_perp(&mut l_amm, &market, &slip(SlippageModel::AmmPriceImpact, "10"),
                 &perp(PerpSide::Long, "500", Some("5"), false));
    assert_eq!(l_amm.perp("hyperliquid", "ETH").unwrap().entry_price, d("2002"));
}
