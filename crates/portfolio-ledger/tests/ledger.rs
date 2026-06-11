//! Tests for the portfolio ledger.

use std::collections::BTreeMap;
use std::str::FromStr;

use catalyst_portfolio_ledger::{Ledger, LedgerError, PerpPosition, PerpSide};
use rust_decimal::Decimal;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn initial(venue: &str, asset: &str, amount: &str) -> Ledger {
    let mut balances = BTreeMap::new();
    let mut a = BTreeMap::new();
    a.insert(asset.to_string(), d(amount));
    balances.insert(venue.to_string(), a);
    Ledger::with_initial(balances, false)
}

// --- Spot balances + credit/debit + negative guard ---

#[test]
fn credit_and_debit_move_balances() {
    let mut l = initial("base", "USDC", "1000");
    l.debit("base", "USDC", d("100")).unwrap();
    l.credit("base", "ETH", d("0.05")).unwrap();
    assert_eq!(l.balance("base", "USDC"), d("900"));
    assert_eq!(l.balance("base", "ETH"), d("0.05"));
}

#[test]
fn strict_ledger_refuses_to_overdraw() {
    let mut l = initial("hyperliquid", "ETH", "0.03");
    let err = l.debit("hyperliquid", "ETH", d("0.04")).unwrap_err();
    assert!(matches!(err, LedgerError::InsufficientBalance { .. }));
    // balance is unchanged after a rejected debit
    assert_eq!(l.balance("hyperliquid", "ETH"), d("0.03"));
}

#[test]
fn allow_negative_ledger_permits_overdraw() {
    let mut l = Ledger::new(true);
    l.debit("base", "USDC", d("50")).unwrap();
    assert_eq!(l.balance("base", "USDC"), d("-50"));
}

// --- Negative-amount guards (#165) ---

#[test]
fn negative_credit_is_rejected() {
    let mut l = initial("base", "USDC", "1000");
    let err = l.credit("base", "USDC", d("-5")).unwrap_err();
    assert!(
        matches!(err, LedgerError::NegativeAmount { op: "credit", .. }),
        "expected NegativeAmount, got {err:?}"
    );
    // balance is unchanged after a rejected credit
    assert_eq!(l.balance("base", "USDC"), d("1000"));
}

#[test]
fn negative_debit_is_rejected_even_under_allow_negative() {
    // A negative debit is a hidden, unguarded credit; allow_negative relaxes
    // only the overdraw guard, never the sign guard.
    for allow_negative in [false, true] {
        let mut l = Ledger::new(allow_negative);
        l.credit("base", "USDC", d("10")).unwrap();
        let err = l.debit("base", "USDC", d("-5")).unwrap_err();
        assert!(
            matches!(err, LedgerError::NegativeAmount { op: "debit", .. }),
            "expected NegativeAmount (allow_negative={allow_negative}), got {err:?}"
        );
        assert_eq!(l.balance("base", "USDC"), d("10"));
    }
}

#[test]
fn zero_credit_and_debit_are_allowed_noops() {
    let mut l = initial("base", "USDC", "100");
    l.credit("base", "USDC", Decimal::ZERO).unwrap();
    l.debit("base", "USDC", Decimal::ZERO).unwrap();
    assert_eq!(l.balance("base", "USDC"), d("100"));
}

#[test]
fn negative_close_perp_settlement_is_rejected() {
    let mut l = initial("hyperliquid", "USDC", "1000");
    l.open_perp(long_eth("100")).unwrap();
    let err = l.close_perp("hyperliquid", "ETH", d("-50")).unwrap_err();
    assert!(
        matches!(err, LedgerError::NegativeAmount { op: "close_perp settlement", .. }),
        "expected NegativeAmount, got {err:?}"
    );
    // Rejected before any mutation: the position is still open and the
    // balance untouched (1000 - 100 margin from the open).
    assert!(l.perp("hyperliquid", "ETH").is_some());
    assert_eq!(l.balance("hyperliquid", "USDC"), d("900"));
}

#[test]
fn negative_withdraw_yield_is_rejected() {
    let mut l = initial("base", "USDC", "100");
    l.deposit_yield("aave", "USDC", "base", Some("usdc"), d("100")).unwrap();
    let err = l.withdraw_yield("aave", "USDC", "base", Some("usdc"), d("-10")).unwrap_err();
    assert!(
        matches!(err, LedgerError::NegativeAmount { op: "withdraw_yield", .. }),
        "expected NegativeAmount, got {err:?}"
    );
    let pos = l.yield_position("aave", "USDC", "base", Some("usdc")).unwrap();
    assert_eq!(pos.principal, d("100"));
    assert_eq!(l.balance("base", "USDC"), Decimal::ZERO);
}

#[test]
fn unknown_balance_is_zero() {
    let l = Ledger::new(false);
    assert_eq!(l.balance("base", "USDC"), Decimal::ZERO);
}

// --- Resting-order reservations (#124) ---

#[test]
fn reserve_excludes_from_available_but_not_balance_or_portfolio() {
    let mut l = initial("base", "USDC", "1000");
    l.reserve("order-1", "base", "USDC", d("600")).unwrap();
    // An earmark, not a debit: the balance — and hence the portfolio/equity —
    // is untouched; only the spendable figure shrinks.
    assert_eq!(l.balance("base", "USDC"), d("1000"));
    assert_eq!(l.reserved("base", "USDC"), d("600"));
    assert_eq!(l.available("base", "USDC"), d("400"));
    assert_eq!(l.to_portfolio(d("0.0125")).balances["base"]["USDC"], "1000");
    // Scoped per (venue, asset): other balances are unaffected.
    assert_eq!(l.reserved("base", "ETH"), Decimal::ZERO);
    assert_eq!(l.reserved("hyperliquid", "USDC"), Decimal::ZERO);
}

#[test]
fn strict_reserve_rejects_overcommit() {
    let mut l = initial("base", "USDC", "1000");
    l.reserve("order-1", "base", "USDC", d("600")).unwrap();
    let err = l.reserve("order-2", "base", "USDC", d("600")).unwrap_err();
    assert!(
        matches!(
            &err,
            LedgerError::InsufficientBalance { available, reserved, .. }
                if *available == d("400") && *reserved == d("600")
        ),
        "expected InsufficientBalance vs available, got {err:?}"
    );
    // The failed reservation left no trace.
    assert_eq!(l.reserved("base", "USDC"), d("600"));
}

#[test]
fn debit_guard_reads_available_and_names_the_reserved_figure() {
    let mut l = initial("base", "USDC", "1000");
    l.reserve("order-1", "base", "USDC", d("600")).unwrap();
    // 500 <= raw balance but > the 400 available -> rejected, naming the earmark.
    let err = l.debit("base", "USDC", d("500")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("have 400 available"), "message reports available: {msg}");
    assert!(msg.contains("600 reserved by resting orders"), "message names reserved: {msg}");
    assert_eq!(l.balance("base", "USDC"), d("1000"));
    // Spending within the available figure still works.
    l.debit("base", "USDC", d("400")).unwrap();
    assert_eq!(l.balance("base", "USDC"), d("600"));
}

#[test]
fn release_frees_available_and_is_idempotent() {
    let mut l = initial("base", "USDC", "1000");
    l.reserve("order-1", "base", "USDC", d("600")).unwrap();
    let released = l.release("order-1").expect("reservation existed");
    assert_eq!(released.venue, "base");
    assert_eq!(released.asset, "USDC");
    assert_eq!(released.amount, d("600"));
    assert_eq!(l.available("base", "USDC"), d("1000"));
    // Idempotent: a second release (or a never-reserved id) is a no-op None.
    assert!(l.release("order-1").is_none());
    assert!(l.release("never-reserved").is_none());
    // The freed cash is spendable again.
    l.debit("base", "USDC", d("1000")).unwrap();
}

#[test]
fn allow_negative_reservations_are_inert() {
    // reserve never fails and the debit stays unguarded — allow_negative keeps
    // its explicit-debt model untouched by #124.
    let mut l = Ledger::new(true);
    l.credit("base", "USDC", d("100")).unwrap();
    l.reserve("order-1", "base", "USDC", d("600")).unwrap(); // over-commit: fine
    l.debit("base", "USDC", d("500")).unwrap(); // unguarded: fine
    assert_eq!(l.balance("base", "USDC"), d("-400"));
}

#[test]
fn negative_reserve_is_rejected_under_every_policy() {
    // #165 discipline: a negative reservation would be a hidden availability
    // credit, so the sign guard holds even under allow_negative.
    for allow_negative in [false, true] {
        let mut l = Ledger::new(allow_negative);
        l.credit("base", "USDC", d("100")).unwrap();
        let err = l.reserve("order-1", "base", "USDC", d("-5")).unwrap_err();
        assert!(
            matches!(err, LedgerError::NegativeAmount { op: "reserve", .. }),
            "expected NegativeAmount (allow_negative={allow_negative}), got {err:?}"
        );
        assert_eq!(l.reserved("base", "USDC"), Decimal::ZERO);
    }
}

#[test]
fn reservations_travel_with_clone() {
    // The engine's trial-commit pattern clones the ledger wholesale; the
    // earmarks must come along or a committed trial would silently drop them.
    let mut l = initial("base", "USDC", "1000");
    l.reserve("order-1", "base", "USDC", d("600")).unwrap();
    let mut clone = l.clone();
    assert_eq!(clone.available("base", "USDC"), d("400"));
    assert!(clone.reservation("order-1").is_some());
    // ...and releasing on the clone doesn't touch the original.
    clone.release("order-1");
    assert_eq!(l.available("base", "USDC"), d("400"));
}

// --- Cost accounting ---

#[test]
fn cost_accumulators_are_signed_and_separate() {
    let mut l = Ledger::new(false);
    l.record_fee(d("0.05"));
    l.record_gas(d("0.02"));
    l.record_funding(d("-1.5")); // received funding
    l.record_yield(d("2.0"));
    assert_eq!(l.fees_usd(), d("0.05"));
    assert_eq!(l.gas_usd(), d("0.02"));
    assert_eq!(l.funding_usd(), d("-1.5"));
    assert_eq!(l.yield_usd(), d("2.0"));
}

// --- Perp position bookkeeping ---

fn long_eth(margin: &str) -> PerpPosition {
    PerpPosition {
        venue: "hyperliquid".to_string(),
        symbol: "ETH".to_string(),
        side: PerpSide::Long,
        size: d("0.25"),
        entry_price: d("2000"),
        leverage: d("5"),
        margin_usd: d(margin),
    }
}

#[test]
fn open_perp_debits_margin_and_records_position() {
    let mut l = initial("hyperliquid", "USDC", "1000");
    l.open_perp(long_eth("100")).unwrap();
    assert_eq!(l.balance("hyperliquid", "USDC"), d("900"));
    let pos = l.perp("hyperliquid", "ETH").unwrap();
    assert_eq!(pos.size, d("0.25"));
    assert_eq!(pos.notional(), d("500"));
}

#[test]
fn open_perp_without_margin_is_rejected() {
    let mut l = initial("hyperliquid", "USDC", "50");
    assert!(matches!(
        l.open_perp(long_eth("100")),
        Err(LedgerError::InsufficientBalance { .. })
    ));
    assert!(l.perp("hyperliquid", "ETH").is_none());
}

#[test]
fn close_perp_credits_settlement_and_removes_position() {
    let mut l = initial("hyperliquid", "USDC", "1000");
    l.open_perp(long_eth("100")).unwrap();
    // settle margin (100) + realized pnl (+25)
    let closed = l.close_perp("hyperliquid", "ETH", d("125")).unwrap();
    assert_eq!(closed.symbol, "ETH");
    assert_eq!(l.balance("hyperliquid", "USDC"), d("1025"));
    assert!(l.perp("hyperliquid", "ETH").is_none());
}

#[test]
fn close_missing_perp_errors() {
    let mut l = Ledger::new(false);
    assert!(matches!(
        l.close_perp("hyperliquid", "ETH", d("0")),
        Err(LedgerError::NoSuchPerp { .. })
    ));
}

#[test]
fn perp_unrealized_pnl_by_side() {
    let long = long_eth("100");
    assert_eq!(long.unrealized_pnl(d("2100")), d("25.00")); // (2100-2000)*0.25
    let short = PerpPosition { side: PerpSide::Short, ..long_eth("100") };
    assert_eq!(short.unrealized_pnl(d("2100")), d("-25.00"));
}

#[test]
fn perp_liquidation_price_by_side() {
    // Long: p_liq = (entry·size − margin) / (size·(1 − mmr));
    // short: p_liq = (entry·size + margin) / (size·(1 + mmr)).  (#120)
    let long = long_eth("100"); // size 0.25, entry 2000, margin 100
    let mmr = d("0.0125");
    assert_eq!(long.liquidation_price(mmr), d("400") / (d("0.25") * d("0.9875")));
    let short = PerpPosition { side: PerpSide::Short, ..long_eth("100") };
    assert_eq!(short.liquidation_price(mmr), d("600") / (d("0.25") * d("1.0125")));

    // At mmr = 0 the level degenerates to the bankruptcy price (loss = margin):
    // equity is exactly zero at the level, i.e. unrealized_pnl == -margin.
    assert_eq!(long.liquidation_price(Decimal::ZERO), d("1600"));
    assert_eq!(long.unrealized_pnl(d("1600")), d("-100.00"));
    assert_eq!(short.liquidation_price(Decimal::ZERO), d("2400"));
    assert_eq!(short.unrealized_pnl(d("2400")), d("-100.00"));

    // At the maintenance level, residual equity == mmr · size · p_liq (up to
    // Decimal's 28-digit division truncation of the repeating p_liq).
    let p_liq = long.liquidation_price(mmr);
    let residual = long.margin_usd + long.unrealized_pnl(p_liq);
    assert!(
        (residual - mmr * long.size * p_liq).abs() < d("0.000000000000000001"),
        "residual {residual} != mmr·size·p_liq {}",
        mmr * long.size * p_liq
    );
}

// --- Yield position bookkeeping ---

#[test]
fn deposit_yield_debits_and_creates_position() {
    let mut l = initial("base", "USDC", "250");
    l.deposit_yield("aave", "USDC", "base", Some("usdc"), d("250")).unwrap();
    assert_eq!(l.balance("base", "USDC"), Decimal::ZERO);
    let pos = l.yield_position("aave", "USDC", "base", Some("usdc")).unwrap();
    assert_eq!(pos.principal, d("250"));
    assert_eq!(pos.accrued, Decimal::ZERO);
}

#[test]
fn accrue_then_withdraw_all_returns_principal_plus_interest() {
    let mut l = initial("base", "USDC", "250");
    l.deposit_yield("aave", "USDC", "base", Some("usdc"), d("250")).unwrap();
    // USDC is a stablecoin, so interest_usd == asset-unit interest (#166).
    l.accrue_yield("aave", "USDC", "base", Some("usdc"), d("1.25"), d("1.25")).unwrap();
    assert_eq!(l.yield_usd(), d("1.25"));

    let all = l.yield_value("aave", "USDC", "base", Some("usdc"));
    assert_eq!(all, d("251.25"));
    let withdrawn = l.withdraw_yield("aave", "USDC", "base", Some("usdc"), all).unwrap();
    assert_eq!(withdrawn, d("251.25"));
    assert_eq!(l.balance("base", "USDC"), d("251.25"));
    // fully redeemed position is removed
    assert!(l.yield_position("aave", "USDC", "base", Some("usdc")).is_none());
}

#[test]
fn partial_withdraw_draws_accrued_first() {
    let mut l = initial("base", "USDC", "250");
    l.deposit_yield("aave", "USDC", "base", Some("usdc"), d("250")).unwrap();
    l.accrue_yield("aave", "USDC", "base", Some("usdc"), d("5"), d("5")).unwrap();
    l.withdraw_yield("aave", "USDC", "base", Some("usdc"), d("3")).unwrap();
    let pos = l.yield_position("aave", "USDC", "base", Some("usdc")).unwrap();
    assert_eq!(pos.accrued, d("2")); // 5 - 3
    assert_eq!(pos.principal, d("250"));
    assert_eq!(l.balance("base", "USDC"), d("3"));
}

#[test]
fn overdraw_yield_is_rejected() {
    let mut l = initial("base", "USDC", "100");
    l.deposit_yield("aave", "USDC", "base", Some("usdc"), d("100")).unwrap();
    assert!(matches!(
        l.withdraw_yield("aave", "USDC", "base", Some("usdc"), d("150")),
        Err(LedgerError::InsufficientYield { .. })
    ));
}

// --- Snapshot ---

#[test]
fn snapshot_reports_balances_positions_and_drops_zeros() {
    let mut l = initial("base", "USDC", "1000");
    l.debit("base", "USDC", d("1000")).unwrap(); // zero it out
    l.credit("hyperliquid", "USDC", d("500")).unwrap();
    l.open_perp(long_eth("100")).unwrap_or(()); // no hyperliquid USDC margin? it has 500
    let portfolio = l.to_portfolio(d("0.0125"));
    // base USDC was zeroed and should be dropped
    assert!(!portfolio.balances.contains_key("base"));
    assert_eq!(portfolio.balances["hyperliquid"]["USDC"], "400");
    assert_eq!(portfolio.perp_positions.len(), 1);
    // The snapshot reports the perp's liquidation price (#120): the level at
    // which equity falls to mmr·notional, no longer a dead `None`.
    let expected = (d("400") / (d("0.25") * d("0.9875"))).normalize().to_string();
    assert_eq!(portfolio.perp_positions[0].liquidation_price.as_deref(), Some(expected.as_str()));
}
