//! Issue #165 — funding-shortfall cascade under strict balance policy.
//!
//! Funding is owed unconditionally, but `Ledger::credit` no longer accepts a
//! negative amount, so a funding charge larger than free cash can't silently
//! overdraw the balance. Under strict policy (`insufficient_balance = reject`,
//! the `strict_v1` default) the engine now cascades the charge:
//!
//!   1. pay from free USDC (clamped at the available balance),
//!   2. deduct the shortfall from the position's posted margin,
//!   3. forgive only the remainder no collateral exists for (true bankruptcy),
//!
//! emitting a `funding_shortfall` event with the exact breakdown. The reduced
//! margin tightens the position's liquidation price, and because
//! `check_liquidations` runs right after `accrue_funding` in the tick loop, a
//! maintenance breach liquidates the position the *same tick*. Under the
//! `allow_negative` policy the historical overdraw-as-margin-debt model is
//! kept: the full charge is debited and the balance goes negative.
//!
//! Common scenario shape: 4h ticks at 0h and 4h, ETH flat at 2000 (BTC flat at
//! 50000 where used), `price_selection = close` + zero fee/slippage overrides
//! so the open fills at tick 0 with exact numbers: size_usd 1000 at 10x =>
//! margin 100, size 0.5, entry 2000, notional 1000. A funding point at 4h with
//! rate r charges payment = r * 1000 at tick 1.

use std::collections::BTreeMap;

use catalyst_contracts::policy::{BalancePolicy, FeePolicy, FillsPolicy, SlippagePolicy};
use catalyst_contracts::trace::Event;
use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const EPOCH: i64 = 1_704_067_200;

fn iso(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Two 4h ticks (0h, 4h). Flat candles per symbol; one funding point per
/// (symbol, rate) at 4h so the charge lands on tick 1.
fn bundle(symbols: &[(&str, &str)], funding_rates: &[(&str, &str)]) -> MarketDataBundle {
    let candles: Vec<Value> = symbols
        .iter()
        .map(|(symbol, px)| {
            let points: Vec<Value> = [0i64, 4]
                .iter()
                .map(|h| json!({"ts": iso(EPOCH + h * 3600), "open": px, "high": px, "low": px, "close": px}))
                .collect();
            json!({"venue": "hyperliquid", "symbol": symbol, "quote": "USD", "points": points})
        })
        .collect();
    let funding: Vec<Value> = funding_rates
        .iter()
        .map(|(symbol, rate)| {
            json!({"venue": "hyperliquid", "symbol": symbol,
                   "points": [{"ts": iso(EPOCH + 4 * 3600), "rate": rate}]})
        })
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "4h", "start": iso(EPOCH), "end": iso(EPOCH + 4 * 3600),
        "candles": candles, "funding": funding,
        "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

fn config(initial_usdc: &str) -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), initial_usdc.to_string());
    let mut init = BTreeMap::new();
    init.insert("hyperliquid".to_string(), bal);
    BacktestConfig {
        start: START.to_string(),
        end: iso(EPOCH + 4 * 3600),
        interval: "4h".to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

/// strict_v1 with same-bar `close` fills and zero fee/slippage, so the opened
/// position has exact round numbers (entry 2000, margin 100, size 0.5).
fn policy() -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None,
        fills: Some(FillsPolicy {
            partial_fills: None,
            price_selection: Some("close".to_string()),
            slippage: Some(SlippagePolicy { model: Some("fixed_bps".to_string()), bps: Some("0".to_string()) }),
            fees: Some(FeePolicy { model: Some("fixed_bps".to_string()), bps: Some("0".to_string()) }),
        }),
        gas: None,
        signals: None,
        ordering: None,
        data: None,
        perps: None,
        yield_: None,
    }
}

/// Same as [`policy`] but with `insufficient_balance = allow_negative`.
fn policy_allow_negative() -> SimulationPolicy {
    let mut p = policy();
    p.balance = Some(BalancePolicy { insufficient_balance: Some("allow_negative".to_string()) });
    p
}

/// One initial 10x long per symbol (size_usd 1000 => margin 100 each).
fn open_longs_graph(symbols: &[&str]) -> Graph {
    let nodes: Vec<Value> = symbols
        .iter()
        .map(|symbol| {
            json!({"id": format!("open_{symbol}"), "kind": "action", "subtype": "perp_order",
                "config": {"symbol": symbol, "side": "long", "size_usd": "1000", "leverage": "10",
                           "chain": "hyperliquid", "order_type": "market", "reduce_only": false}})
        })
        .collect();
    serde_json::from_value(json!({ "nodes": nodes, "edges": [] })).unwrap()
}

fn run_case(
    initial_usdc: &str,
    symbols: &[(&str, &str)],
    funding_rates: &[(&str, &str)],
    policy_: SimulationPolicy,
) -> SimulationTrace {
    run(&SimulationInput {
        graph: open_longs_graph(&symbols.iter().map(|(s, _)| *s).collect::<Vec<_>>()),
        config: config(initial_usdc),
        policy: policy_,
        market_data: bundle(symbols, funding_rates),
    })
    .unwrap()
}

fn events_of<'a>(t: &'a SimulationTrace, event_type: &str) -> Vec<&'a Event> {
    t.events.iter().filter(|e| e.event_type == event_type).collect()
}

fn detail_str<'a>(e: &'a Event, key: &str) -> &'a str {
    e.detail.as_ref().unwrap()[key].as_str().unwrap_or_else(|| panic!("missing detail {key}"))
}

fn final_usdc(t: &SimulationTrace) -> String {
    t.final_portfolio
        .balances
        .get("hyperliquid")
        .and_then(|a| a.get("USDC"))
        .cloned()
        .unwrap_or_else(|| "0".to_string()) // zero balances are dropped from snapshots
}

/// Funding (5) exceeds free cash (2): 2 paid from cash, 3 deducted from
/// margin, nothing forgiven. The position survives with margin 97 (still far
/// above the 12.5 maintenance level at mmr 0.0125), and cash lands exactly at
/// zero — never negative.
#[test]
fn shortfall_cascades_from_cash_into_margin() {
    let trace = run_case("102", &[("ETH", "2000")], &[("ETH", "0.005")], policy());

    let shortfalls = events_of(&trace, "funding_shortfall");
    assert_eq!(shortfalls.len(), 1, "expected exactly one funding_shortfall event");
    let s = shortfalls[0];
    assert_eq!(s.ts, iso(EPOCH + 4 * 3600));
    assert_eq!(detail_str(s, "venue"), "hyperliquid");
    assert_eq!(detail_str(s, "symbol"), "ETH");
    assert_eq!(detail_str(s, "payment"), "5");
    assert_eq!(detail_str(s, "paid_cash"), "2");
    assert_eq!(detail_str(s, "from_margin"), "3");
    assert_eq!(detail_str(s, "forgiven"), "0");

    // Nothing was forgiven, so the full charge counts as collected.
    let applied = events_of(&trace, "funding_applied");
    assert_eq!(applied.len(), 1);
    assert_eq!(detail_str(applied[0], "payment_usd"), "5");
    assert_eq!(detail_str(applied[0], "collected_usd"), "5");

    // Cash exactly zero, margin reduced by the shortfall, position survives.
    assert_eq!(final_usdc(&trace), "0");
    assert!(events_of(&trace, "liquidation").is_empty());
    let perps = &trace.final_portfolio.perp_positions;
    assert_eq!(perps.len(), 1);
    assert_eq!(perps[0].margin_usd.as_deref(), Some("97"));
}

/// Exactly-zero cash: the whole charge comes out of margin (paid_cash 0).
#[test]
fn zero_cash_pays_funding_entirely_from_margin() {
    // Initial 100 — the open consumes all of it as margin; cash is exactly 0.
    let trace = run_case("100", &[("ETH", "2000")], &[("ETH", "0.005")], policy());

    let shortfalls = events_of(&trace, "funding_shortfall");
    assert_eq!(shortfalls.len(), 1);
    let s = shortfalls[0];
    assert_eq!(detail_str(s, "payment"), "5");
    assert_eq!(detail_str(s, "paid_cash"), "0");
    assert_eq!(detail_str(s, "from_margin"), "5");
    assert_eq!(detail_str(s, "forgiven"), "0");

    assert_eq!(final_usdc(&trace), "0");
    let perps = &trace.final_portfolio.perp_positions;
    assert_eq!(perps.len(), 1);
    assert_eq!(perps[0].margin_usd.as_deref(), Some("95"));
}

/// The margin deduction breaches maintenance (strict default mmr 0.0125) and
/// the position is liquidated the *same tick*: funding payment 92 leaves
/// margin 10 < 12.5 maintenance on the 1000 notional, p_liq tightens to
/// 990 / (0.5 * 0.9875) ~ 2005 > the flat 2000 mark, and `check_liquidations`
/// (which runs right after `accrue_funding`) closes it, settling the residual
/// margin 10 back to cash.
#[test]
fn margin_deduction_breaching_maintenance_liquidates_same_tick() {
    let trace = run_case("102", &[("ETH", "2000")], &[("ETH", "0.092")], policy());

    let shortfalls = events_of(&trace, "funding_shortfall");
    assert_eq!(shortfalls.len(), 1);
    let s = shortfalls[0];
    assert_eq!(detail_str(s, "payment"), "92");
    assert_eq!(detail_str(s, "paid_cash"), "2");
    assert_eq!(detail_str(s, "from_margin"), "90");
    assert_eq!(detail_str(s, "forgiven"), "0");

    let liquidations = events_of(&trace, "liquidation");
    assert_eq!(liquidations.len(), 1, "maintenance breach must liquidate");
    // Same tick as the funding shortfall — no extra tick of phantom survival.
    assert_eq!(liquidations[0].ts, s.ts);
    assert_eq!(detail_str(liquidations[0], "settled_usd"), "10");

    // Cash: 2 paid to funding, then the residual margin 10 settles back.
    assert_eq!(final_usdc(&trace), "10");
    assert!(trace.final_portfolio.perp_positions.is_empty());
}

/// True bankruptcy: the charge (150) exceeds cash (2) + margin (100); the
/// uncollectable remainder (48) is forgiven — the only case `forgiven` is
/// non-zero — and `collected_usd` reports only the 102 that actually moved.
/// The margin-stripped position is liquidated the same tick with nothing left
/// to settle. Ledger-delta reconciliation: the entire initial 102 is accounted
/// for by collected funding (102 = 2 cash + 100 margin), settlements 0.
#[test]
fn bankrupt_funding_forgives_remainder_and_liquidates_with_zero_residual() {
    let trace = run_case("102", &[("ETH", "2000")], &[("ETH", "0.15")], policy());

    let shortfalls = events_of(&trace, "funding_shortfall");
    assert_eq!(shortfalls.len(), 1);
    let s = shortfalls[0];
    assert_eq!(detail_str(s, "payment"), "150");
    assert_eq!(detail_str(s, "paid_cash"), "2");
    assert_eq!(detail_str(s, "from_margin"), "100");
    assert_eq!(detail_str(s, "forgiven"), "48");

    // The funding accumulator reflects money that moved, not the forgiven part.
    let applied = events_of(&trace, "funding_applied");
    assert_eq!(applied.len(), 1);
    assert_eq!(detail_str(applied[0], "payment_usd"), "150");
    assert_eq!(detail_str(applied[0], "collected_usd"), "102");

    // Margin 0 => p_liq = entry / (1 - mmr) > mark => liquidated same tick,
    // settling exactly nothing.
    let liquidations = events_of(&trace, "liquidation");
    assert_eq!(liquidations.len(), 1);
    assert_eq!(liquidations[0].ts, s.ts);
    assert_eq!(detail_str(liquidations[0], "settled_usd"), "0");

    // Reconciliation: initial 102 - final 0 == funding collected 102
    // (margin posted 100 all consumed, settlement 0, fees 0).
    assert_eq!(final_usdc(&trace), "0");
    assert!(trace.final_portfolio.perp_positions.is_empty());
}

/// Two positions on the same venue, same tick: deterministic (venue, symbol)
/// order means BTC settles its funding from cash first, and ETH — processed
/// second — sees the drained balance and cascades into its own margin. BTC's
/// margin is untouched.
#[test]
fn second_position_same_tick_sees_drained_cash() {
    // Initial 203: two opens take 100 margin each, leaving cash 3.
    // BTC: payment 2 (covered by cash, leaves 1). ETH: payment 2 -> 1 from
    // cash, 1 from ETH margin.
    let trace = run_case(
        "203",
        &[("BTC", "50000"), ("ETH", "2000")],
        &[("BTC", "0.002"), ("ETH", "0.002")],
        policy(),
    );

    let shortfalls = events_of(&trace, "funding_shortfall");
    assert_eq!(shortfalls.len(), 1, "only the later (ETH) position should fall short");
    let s = shortfalls[0];
    assert_eq!(detail_str(s, "symbol"), "ETH");
    assert_eq!(detail_str(s, "payment"), "2");
    assert_eq!(detail_str(s, "paid_cash"), "1");
    assert_eq!(detail_str(s, "from_margin"), "1");
    assert_eq!(detail_str(s, "forgiven"), "0");

    assert_eq!(final_usdc(&trace), "0");
    let perps = &trace.final_portfolio.perp_positions;
    assert_eq!(perps.len(), 2);
    let margin_of = |symbol: &str| {
        perps.iter().find(|p| p.symbol == symbol).unwrap().margin_usd.as_deref().map(str::to_string)
    };
    assert_eq!(margin_of("BTC"), Some("100".to_string()), "BTC paid from cash; margin intact");
    assert_eq!(margin_of("ETH"), Some("99".to_string()), "ETH cascaded 1 into margin");
}

/// Regression pin: under `insufficient_balance = allow_negative` the explicit
/// margin-debt model is kept — the full charge overdraws the balance, no
/// cascade, no shortfall event, margin untouched.
#[test]
fn allow_negative_policy_still_overdraws_instead_of_cascading() {
    let trace =
        run_case("102", &[("ETH", "2000")], &[("ETH", "0.005")], policy_allow_negative());

    assert!(events_of(&trace, "funding_shortfall").is_empty());
    let applied = events_of(&trace, "funding_applied");
    assert_eq!(applied.len(), 1);
    assert_eq!(detail_str(applied[0], "payment_usd"), "5");
    assert_eq!(detail_str(applied[0], "collected_usd"), "5");

    // Cash 2 - 5 = -3: the overdraw is explicit debt under this policy.
    assert_eq!(final_usdc(&trace), "-3");
    let perps = &trace.final_portfolio.perp_positions;
    assert_eq!(perps.len(), 1);
    assert_eq!(perps[0].margin_usd.as_deref(), Some("100"));
    assert!(events_of(&trace, "liquidation").is_empty());
}

/// Receiving funding (short + positive rate) is unaffected by the cascade: a
/// plain credit, no shortfall machinery.
#[test]
fn receiving_funding_still_credits_cash() {
    let graph: Graph = serde_json::from_value(json!({
        "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
            "config": {"symbol": "ETH", "side": "short", "size_usd": "1000", "leverage": "10",
                       "chain": "hyperliquid", "order_type": "market", "reduce_only": false}}],
        "edges": []
    }))
    .unwrap();
    let trace = run(&SimulationInput {
        graph,
        config: config("102"),
        policy: policy(),
        market_data: bundle(&[("ETH", "2000")], &[("ETH", "0.005")]),
    })
    .unwrap();

    assert!(events_of(&trace, "funding_shortfall").is_empty());
    let applied = events_of(&trace, "funding_applied");
    assert_eq!(applied.len(), 1);
    // Short receives positive funding: payment is negative (we are paid).
    assert_eq!(detail_str(applied[0], "payment_usd"), "-5");
    assert_eq!(detail_str(applied[0], "collected_usd"), "-5");
    // Cash 2 + 5 received = 7.
    assert_eq!(final_usdc(&trace), "7");
}
