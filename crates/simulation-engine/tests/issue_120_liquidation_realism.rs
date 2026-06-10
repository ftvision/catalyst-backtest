//! Issue #120 — liquidation realism. BOTH halves are now FIXED:
//!
//!   (a) INTRABAR WICK MARKING (landed in #152): `check_liquidations` marks each
//!       perp at the worst price it touches *within* the bar (a long at the bar
//!       LOW, a short at the bar HIGH), so a position that breaches intrabar can
//!       no longer escape just because it recovers by the close.
//!   (b) MAINTENANCE MARGIN (this file's flip): liquidation triggers at the
//!       *maintenance* level — the mark where equity (margin + unrealized PnL)
//!       falls to `perps.maintenance_margin_ratio` of mark notional — not at
//!       full bankruptcy. The position settles its residual equity at the fill
//!       price (the liquidation price, or the bar's open when the bar gapped
//!       through it), floored at zero.
//!
//! Liquidation price (`PerpPosition::liquidation_price`):
//!
//!   long:  p_liq = (entry·size − margin) / (size·(1 − mmr))
//!   short: p_liq = (entry·size + margin) / (size·(1 + mmr))
//!
//! Default ratio: 0.0125 (Hyperliquid's top-tier maintenance margin, 1/(2·40x)).
//! `maintenance_margin_ratio = "0"` degenerates to the historical bankruptcy
//! trigger, pinned below.
//!
//! Standard long scenario (strict_v1, NextOpen): entry 2002 = 2000·1.001,
//! size = 1000/2002 ≈ 0.4995, margin 100 ⇒
//!   p_liq = (2002·size − 100)/(size·0.9875) = 900·2002/987.5 ≈ 1824.6076
//!   bankruptcy price (mmr=0) = 1801.8.

use std::collections::BTreeMap;
use std::str::FromStr;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use rust_decimal::Decimal;
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const EPOCH: i64 = 1_704_067_200;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn iso(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Build a 3-bar (4h) ETH market with the given (open, high, low, close) per tick.
fn bundle(bars: [(&str, &str, &str, &str); 3]) -> MarketDataBundle {
    let candles: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| {
            json!({"ts": iso(EPOCH + (i as i64) * 4 * 3600), "open": o, "high": h, "low": l, "close": c})
        })
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "4h", "start": iso(EPOCH), "end": iso(EPOCH + 8 * 3600),
        "candles": [{"venue": "hyperliquid", "symbol": "ETH", "quote": "USD", "points": candles}],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

fn config() -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), "2000".to_string());
    let mut init = BTreeMap::new();
    init.insert("hyperliquid".to_string(), bal);
    BacktestConfig {
        start: START.to_string(),
        end: iso(EPOCH + 8 * 3600),
        interval: "4h".to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

fn policy() -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None, fills: None, gas: None, signals: None, ordering: None,
        data: None, perps: None, yield_: None,
    }
}

/// strict_v1 but with an explicit `perps.maintenance_margin_ratio` override.
fn policy_with_mmr(ratio: &str) -> SimulationPolicy {
    let mut p = policy();
    p.perps = Some(catalyst_contracts::policy::PerpPolicy {
        liquidation_check: None,
        funding: None,
        reduce_only_validation: None,
        maintenance_margin_ratio: Some(ratio.to_string()),
    });
    p
}

/// Open a 10x long ETH perp at tick0. strict_v1 => NextOpen fill at tick1.open*1.001.
/// size_usd=1000, leverage=10 => margin_usd=100, entry_price=2002, size_base=0.4995004995...
fn open_long_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
            "config": {"symbol": "ETH", "side": "long", "size_usd": "1000", "leverage": "10",
                       "chain": "hyperliquid", "order_type": "market", "reduce_only": false}}],
        "edges": []
    }))
    .unwrap()
}

/// Open a 10x SHORT ETH perp at tick0. strict_v1 => NextOpen fill at tick1.open*0.999
/// (sell slippage). size_usd=1000, leverage=10 => margin_usd=100, entry_price=1998,
/// size_base=1000/1998=0.5005005005...
fn open_short_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
            "config": {"symbol": "ETH", "side": "short", "size_usd": "1000", "leverage": "10",
                       "chain": "hyperliquid", "order_type": "market", "reduce_only": false}}],
        "edges": []
    }))
    .unwrap()
}

/// The long scenario's exact numbers, mirroring the engine's Decimal ops:
/// size = 1000/2002, entry 2002, margin 100, mmr 0.0125.
fn long_p_liq() -> (Decimal, Decimal) {
    let size = d("1000") / d("2002");
    let p_liq = (d("2002") * size - d("100")) / (size * (Decimal::ONE - d("0.0125")));
    (size, p_liq)
}

fn liquidation_detail(trace: &catalyst_contracts::trace::SimulationTrace) -> &Value {
    trace
        .events
        .iter()
        .find(|e| e.event_type == "liquidation")
        .expect("a liquidation event")
        .detail
        .as_ref()
        .expect("liquidation detail")
}

/// Issue #120 sub-bug (a) — FIXED: a long is liquidated on the intrabar LOW even
/// when the bar recovers by its CLOSE.
///
/// The position is open by tick1 (NextOpen fill, entry 2002, size 0.4995004995,
/// margin 100). At tick2 LOW=1700 < p_liq 1824.61 => LIQUIDATE, even though
/// CLOSE=2000 would not. Close-only marking (the old bug) would have let it
/// escape. Under the maintenance model the breach is *not* a gap (open 2000 >
/// p_liq), so the position settles its residual at p_liq instead of losing the
/// full margin.
#[test]
fn issue_120_long_liquidated_on_intrabar_wick() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"), // open fills here (NextOpen)
            ("2000", "2000", "1700", "2000"), // wick to 1700, recovers to close 2000
        ]),
    })
    .unwrap();

    assert!(
        trace.events.iter().any(|e| e.event_type == "liquidation"),
        "issue #120(a) FIXED: the long must be liquidated on the bar LOW (1700 < p_liq \
         1824.61) even though the close (2000) recovers."
    );
    assert!(
        !trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "issue #120(a) FIXED: the liquidated ETH long must be gone from final_portfolio."
    );
    // Maintenance model (#120(b)): no gap through p_liq (open 2000 > p_liq), so
    // the residual settles at p_liq — a positive amount, not the old hard zero.
    let detail = liquidation_detail(&trace);
    let settled = d(detail["settled_usd"].as_str().unwrap());
    assert!(settled > Decimal::ZERO, "wick liquidation settles a residual, got {settled}");
}

/// Issue #120 sub-bug (a) — FIXED, SHORT side: a short is liquidated on the
/// intrabar HIGH even when the bar recovers by its CLOSE.
///
/// Short entry 1998 (sell slippage), size 0.5005005005, margin 100.
/// p_liq = (1998·size + 100)/(size·1.0125) = 1100·1998/1012.5 ≈ 2170.67. At tick2
/// HIGH=2300 >= p_liq => LIQUIDATE, though the close (2000) would not.
#[test]
fn issue_120_short_liquidated_on_intrabar_wick() {
    let trace = run(&SimulationInput {
        graph: open_short_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"), // open fills here (NextOpen)
            ("2000", "2300", "2000", "2000"), // spikes to 2300, recovers to close 2000
        ]),
    })
    .unwrap();

    assert!(
        trace.events.iter().any(|e| e.event_type == "liquidation"),
        "issue #120(a) FIXED (short): liquidate on the bar HIGH (2300 >= p_liq 2170.67)."
    );
    assert!(
        !trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "issue #120(a) FIXED (short): the liquidated ETH short must be gone."
    );
}

/// Issue #120 — guard against OVER-liquidation: a wick that does NOT reach the
/// liquidation price must leave the position open. Long entry 2002, margin 100,
/// p_liq ≈ 1824.61; tick2 LOW=1900 > p_liq => NO liquidation (even though the
/// maintenance trigger fires well before the old bankruptcy level 1801.8).
#[test]
fn issue_120_no_liquidation_when_wick_does_not_reach_liquidation_price() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "1900", "2000"), // shallow wick: 1900 > p_liq 1824.61
        ]),
    })
    .unwrap();

    assert!(
        trace.events.iter().all(|e| e.event_type != "liquidation"),
        "issue #120: a wick to 1900 (> p_liq 1824.61) must NOT liquidate."
    );
    assert!(
        trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "issue #120: the ETH long must survive a non-breaching wick."
    );
}

/// Issue #120 sub-bug (b) — FLIPPED from the recorded defect: the maintenance
/// buffer liquidates a near-margin-floor position that the old full-bankruptcy
/// trigger let survive.
///
/// Entry 2002, size = 1000/2002, margin 100 ⇒ p_liq = 900·2002/987.5 ≈ 1824.61.
/// The market sits at 1802 from tick1 on — *above* the bankruptcy price 1801.8
/// (pnl −99.9001 > −100, so the old trigger never fired; the recorded test
/// asserted survival) but *below* p_liq, so the maintenance model liquidates.
/// The first check that sees the open position is tick2 (the deferred NextOpen
/// fill books at tick1 *after* that tick's liquidation pass), whose bar opens at
/// 1802 < p_liq — a gap through the level — so the fill is the open (1802) and
/// the residual is margin + pnl(1802) = 100 − 200·(1000/2002) ≈ 0.0999, not the
/// no-gap residual mmr·size·p_liq.
#[test]
fn issue_120_long_liquidated_at_maintenance_level_before_full_bankruptcy() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "1802", "1802"), // drops to 1802: below p_liq, above bankruptcy
            ("1802", "1802", "1802", "1802"),
        ]),
    })
    .unwrap();

    let (size, p_liq) = long_p_liq();
    // Sanity on the scenario itself: 1802 is between bankruptcy and p_liq.
    assert!(d("1801.8") < d("1802") && d("1802") < p_liq, "p_liq = {p_liq}");

    let detail = liquidation_detail(&trace);
    assert_eq!(detail["liquidation_price"].as_str().unwrap(), p_liq.normalize().to_string());
    assert_eq!(detail["mark"].as_str().unwrap(), "1802", "gap-open below p_liq fills at the open");

    // Residual settled back: margin + pnl(1802), computed exactly.
    let expected = d("100") + (d("1802") - d("2002")) * size;
    assert!(expected > Decimal::ZERO);
    assert_eq!(detail["settled_usd"].as_str().unwrap(), expected.normalize().to_string());
    assert_eq!(
        detail["margin_lost_usd"].as_str().unwrap(),
        (d("100") - expected).normalize().to_string()
    );

    assert!(
        !trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "issue #120(b) FIXED: the maintenance model must liquidate at 1802 (≤ p_liq \
         {p_liq:.4}), where the old bankruptcy trigger (mark ≤ 1801.8) did not."
    );
}

/// No-gap breach settles EXACTLY the maintenance residual: when the bar's open
/// is above p_liq and only the wick crosses it, the fill is p_liq itself and the
/// settlement is margin + pnl(p_liq) — algebraically mmr·size·p_liq.
#[test]
fn issue_120_no_gap_breach_settles_residual_at_liquidation_price() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "1800", "2000"), // open 2000 > p_liq, wick 1800 <= p_liq
        ]),
    })
    .unwrap();

    let (size, p_liq) = long_p_liq();
    let detail = liquidation_detail(&trace);
    assert_eq!(detail["mark"].as_str().unwrap(), p_liq.normalize().to_string());
    assert_eq!(detail["liquidation_price"].as_str().unwrap(), p_liq.normalize().to_string());

    let expected = d("100") + (p_liq - d("2002")) * size; // engine's exact expression
    assert_eq!(detail["settled_usd"].as_str().unwrap(), expected.normalize().to_string());

    // ... and that equals mmr·size·p_liq (up to Decimal's 28-digit truncation
    // of the repeating division), the maintenance residual by construction.
    let identity = d("0.0125") * size * p_liq;
    assert!(
        (expected - identity).abs() < d("0.000000000000000001"),
        "settlement {expected} != mmr·size·p_liq {identity}"
    );
}

/// A bar that gaps through the BANKRUPTCY price settles zero (regression of the
/// old behavior at its own boundary): fill = min(open, p_liq) = open, and the
/// residual margin + pnl(open) is negative, so it clamps at zero — a liquidation
/// can never claw back collateral that was never posted (#117 invariant).
#[test]
fn issue_120_gap_through_bankruptcy_clamps_settlement_to_zero() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"),
            ("1500", "1500", "1500", "1500"), // gap open far below bankruptcy (1801.8)
        ]),
    })
    .unwrap();

    let detail = liquidation_detail(&trace);
    assert_eq!(detail["mark"].as_str().unwrap(), "1500", "gap fills at the open");
    assert_eq!(detail["settled_usd"].as_str().unwrap(), "0");
    assert_eq!(detail["margin_lost_usd"].as_str().unwrap(), "100");
    assert!(!trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"));
}

/// Short-side mirror of the no-gap breach: entry 1998, size = 1000/1998,
/// p_liq = (1998·size + 100)/(size·1.0125) = 1100·1998/1012.5 ≈ 2170.67. A wick
/// to 2200 (open 2000 < p_liq) fills at p_liq and settles the exact residual.
#[test]
fn issue_120_short_no_gap_breach_settles_residual_at_liquidation_price() {
    let trace = run(&SimulationInput {
        graph: open_short_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"),
            ("2000", "2200", "2000", "2000"), // open 2000 < p_liq ≈ 2170.67, high 2200 >= p_liq
        ]),
    })
    .unwrap();

    let size = d("1000") / d("1998");
    let p_liq = (d("1998") * size + d("100")) / (size * (Decimal::ONE + d("0.0125")));

    let detail = liquidation_detail(&trace);
    assert_eq!(detail["mark"].as_str().unwrap(), p_liq.normalize().to_string());
    assert_eq!(detail["liquidation_price"].as_str().unwrap(), p_liq.normalize().to_string());

    let expected = d("100") + (d("1998") - p_liq) * size;
    assert_eq!(detail["settled_usd"].as_str().unwrap(), expected.normalize().to_string());
    let identity = d("0.0125") * size * p_liq;
    assert!(
        (expected - identity).abs() < d("0.000000000000000001"),
        "settlement {expected} != mmr·size·p_liq {identity}"
    );
    assert!(!trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"));
}

/// Degenerate pin: `perps.maintenance_margin_ratio = "0"` reproduces the exact
/// pre-#120 bankruptcy behavior — at mark 1802 (pnl −99.9001, just inside the
/// −100 margin floor) the position SURVIVES, exactly as the old recorded-defect
/// test asserted.
#[test]
fn issue_120_zero_ratio_degenerates_to_old_bankruptcy_trigger() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy_with_mmr("0"),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "1802", "1802"), // pnl −99.9001 > −100: no bankruptcy
            ("1802", "1802", "1802", "1802"),
        ]),
    })
    .unwrap();

    assert!(
        trace.events.iter().all(|e| e.event_type != "liquidation"),
        "mmr=0 must reproduce the old full-bankruptcy trigger: pnl −99.9001 > −100 ⇒ survive"
    );
    assert!(
        trace.final_portfolio.perp_positions.iter().any(|p| p.symbol == "ETH"),
        "mmr=0: the ETH long must survive right up to the margin floor (old behavior, pinned)"
    );
}

/// ... and with mmr=0 a mark AT the bankruptcy floor still liquidates, settling
/// zero — the boundary itself is unchanged.
#[test]
fn issue_120_zero_ratio_still_liquidates_at_bankruptcy_settling_zero() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy_with_mmr("0"),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "1801.8", "2000"), // exactly the bankruptcy price
        ]),
    })
    .unwrap();

    let detail = liquidation_detail(&trace);
    assert_eq!(detail["settled_usd"].as_str().unwrap(), "0");
    assert_eq!(detail["margin_lost_usd"].as_str().unwrap(), "100");
}

/// The portfolio snapshot reports each open perp's liquidation price (#120):
/// the previously dead `liquidation_price` field is populated from the policy's
/// maintenance ratio.
#[test]
fn issue_120_snapshot_reports_liquidation_price() {
    let trace = run(&SimulationInput {
        graph: open_long_graph(),
        config: config(),
        policy: policy(),
        market_data: bundle([
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "2000", "2000"),
            ("2000", "2000", "1900", "2000"), // survives (1900 > p_liq)
        ]),
    })
    .unwrap();

    let (_, p_liq) = long_p_liq();
    let pos = trace
        .final_portfolio
        .perp_positions
        .iter()
        .find(|p| p.symbol == "ETH")
        .expect("ETH long survives");
    assert_eq!(
        pos.liquidation_price.as_deref(),
        Some(p_liq.normalize().to_string().as_str()),
        "final_portfolio must report the maintenance liquidation price"
    );
}
