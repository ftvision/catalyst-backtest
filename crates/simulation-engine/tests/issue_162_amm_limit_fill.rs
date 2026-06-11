//! #162: resting limit orders are MAKER orders under `amm_price_impact`.
//!
//! A resting limit fills at limit-or-better (the gap-aware touched price); the
//! constant-product AMM impact model never reprices the fill — previously a buy
//! limit at 1900 could fill at 2020 when the pool was shallow. For honesty the
//! theoretical taker price is emitted in the fill detail as
//! `amm_theoretical_price`, with `amm_impact_exceeds_limit = true` when that
//! theoretical price is worse than the actual fill from the trader's
//! perspective.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const START_EPOCH: i64 = 1_704_067_200;
const STEP: i64 = 3600;

fn ts(i: i64) -> String {
    let epoch = START_EPOCH + i * STEP;
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A bundle with an ETH candle path of explicit (open, high, low, close) bars on
/// `base`, plus a pool-reserves (liquidity) series fixed at bar 0.
fn bundle(bars: &[(&str, &str, &str, &str)], reserves: Option<(&str, &str)>) -> MarketDataBundle {
    let points: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| json!({"ts": ts(i as i64), "open": o, "high": h, "low": l, "close": c}))
        .collect();
    let mut b = json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h",
        "start": ts(0),
        "end": ts(bars.len() as i64),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": points}],
        "gas": [], "funding": [], "yields": [], "providers": [], "warnings": []
    });
    if let Some((rb, rq)) = reserves {
        b["liquidity"] = json!([{"venue": "base", "symbol": "ETH", "points": [
            {"ts": ts(0), "reserve_base": rb, "reserve_quote": rq}
        ]}]);
    }
    serde_json::from_value(b).unwrap()
}

fn config(asset: &str, amount: &str, n_ticks: i64) -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert(asset.to_string(), amount.to_string());
    let mut init = BTreeMap::new();
    init.insert("base".to_string(), bal);
    BacktestConfig {
        start: START.to_string(),
        // Last bar is ts(n_ticks - 1); #167 enforces the window matches the data.
        end: ts(n_ticks - 1),
        interval: "1h".to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

/// strict_v1 with the slippage model overridden to amm_price_impact.
fn amm_policy() -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "fills": { "slippage": { "model": "amm_price_impact" } }
    }))
    .unwrap()
}

fn limit_swap_graph(from: &str, to: &str, amount: &str, limit: &str) -> Graph {
    serde_json::from_value(json!({
        "nodes": [{"id": "order", "kind": "action", "subtype": "swap",
            "config": {"from_asset": from, "to_asset": to, "amount": amount, "chain": "base",
                       "order_type": "limit", "limit_price": limit}}],
        "edges": []
    }))
    .unwrap()
}

fn event_detail<'a>(trace: &'a SimulationTrace, kind: &str) -> &'a Value {
    trace
        .events
        .iter()
        .find(|e| e.event_type == kind)
        .and_then(|e| e.detail.as_ref())
        .unwrap_or_else(|| panic!("no {kind} event with detail"))
}

fn num(detail: &Value, key: &str) -> f64 {
    detail
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("missing {key} in {detail}"))
        .parse()
        .unwrap()
}

// --- (1) shallow pool: the limit holds; the theoretical impact is emitted ---

#[test]
fn buy_limit_shallow_pool_fills_at_limit_with_honest_impact_fields() {
    // Buy limit at 1900 placed on bar 0, touched on bar 1 (low 1850, open 1980).
    // Shallow pool 100 ETH / 200k USDC: buying 2000 USDC as a taker would average
    // (200000+2000)/100 = 2020 — pre-fix the fill price. Maker semantics: fills
    // AT the limit, 1900, and emits the 2020 theoretical with the exceeds flag.
    let trace = run(&SimulationInput {
        graph: limit_swap_graph("USDC", "ETH", "2000", "1900"),
        config: config("USDC", "5000", 2),
        policy: amm_policy(),
        market_data: bundle(
            &[("2000", "2010", "1990", "2000"), ("1980", "1985", "1850", "1900")],
            Some(("100", "200000")),
        ),
    })
    .unwrap();
    let detail = event_detail(&trace, "order_filled");
    assert_eq!(num(detail, "price"), 1900.0, "maker fill at the limit, not the AMM price");
    assert_eq!(num(detail, "amm_theoretical_price"), 2020.0);
    assert_eq!(detail.get("amm_impact_exceeds_limit"), Some(&json!(true)));
}

// --- (2) deep pool, favorable theoretical: maker, not clamp ---

#[test]
fn buy_limit_deep_pool_still_fills_at_maker_price_not_favorable_amm() {
    // Pool mid 1850 (1000 ETH / 1.85M USDC): the theoretical taker price for a
    // 2000-USDC buy is (1850000+2000)/1000 = 1852, BETTER than the 1900 limit.
    // Maker semantics still fill at the engine maker price (1900, since the bar
    // opens at 1980 and dips through) — the favorable AMM price is NOT
    // substituted. This pins "maker", not "clamp at the limit".
    let trace = run(&SimulationInput {
        graph: limit_swap_graph("USDC", "ETH", "2000", "1900"),
        config: config("USDC", "5000", 2),
        policy: amm_policy(),
        market_data: bundle(
            &[("2000", "2010", "1990", "2000"), ("1980", "1985", "1850", "1900")],
            Some(("1000", "1850000")),
        ),
    })
    .unwrap();
    let detail = event_detail(&trace, "order_filled");
    assert_eq!(num(detail, "price"), 1900.0, "maker fill, not the favorable AMM 1852");
    assert_eq!(num(detail, "amm_theoretical_price"), 1852.0);
    assert_eq!(detail.get("amm_impact_exceeds_limit"), Some(&json!(false)));
}

// --- (3) sell-side mirror of (1) ---

#[test]
fn sell_limit_shallow_pool_fills_at_limit_with_honest_impact_fields() {
    // Sell limit at 2100, touched on bar 1 (high 2150, open 2050). Selling 1 ETH
    // into the shallow pool as a taker would average 200000/101 ≈ 1980.2 — far
    // below the limit. Maker semantics: fills AT 2100; theoretical emitted, and
    // exceeds is true (theoretical < fill for a sell = worse for the trader).
    let trace = run(&SimulationInput {
        graph: limit_swap_graph("ETH", "USDC", "1", "2100"),
        config: config("ETH", "5", 2),
        policy: amm_policy(),
        market_data: bundle(
            &[("2000", "2010", "1990", "2000"), ("2050", "2150", "2040", "2100")],
            Some(("100", "200000")),
        ),
    })
    .unwrap();
    let detail = event_detail(&trace, "order_filled");
    assert_eq!(num(detail, "price"), 2100.0, "maker fill at the limit, not the AMM price");
    let theo = num(detail, "amm_theoretical_price");
    assert!((1980.0..1981.0).contains(&theo), "theoretical was {theo}");
    assert_eq!(detail.get("amm_impact_exceeds_limit"), Some(&json!(true)));
}

// --- (4) gap-through still fills at the (better) open, reserves present ---

#[test]
fn gap_through_with_reserves_fills_at_open() {
    // Bar 1 gaps open at 1850, below the 1900 buy limit: the gap-aware
    // limit-or-better price is the open, 1850 — and it survives under
    // amm_price_impact with reserves present (no AMM override of the open fill).
    let trace = run(&SimulationInput {
        graph: limit_swap_graph("USDC", "ETH", "2000", "1900"),
        config: config("USDC", "5000", 2),
        policy: amm_policy(),
        market_data: bundle(
            &[("2000", "2010", "1990", "2000"), ("1850", "1860", "1820", "1840")],
            Some(("100", "200000")),
        ),
    })
    .unwrap();
    let detail = event_detail(&trace, "order_filled");
    assert_eq!(num(detail, "price"), 1850.0, "gap-through fills at the open");
    assert_eq!(num(detail, "amm_theoretical_price"), 2020.0);
    assert_eq!(detail.get("amm_impact_exceeds_limit"), Some(&json!(true)));
}

// --- (5) regression: MARKET swaps still get AMM impact ---

#[test]
fn market_swaps_still_get_amm_impact() {
    // Companion to amm_slippage.rs: the taker path is unchanged — a market buy of
    // 2000 USDC into the shallow pool fills at the constant-product 2020, and the
    // honesty fields are absent (they are resting-limit-only).
    let graph: Graph = serde_json::from_value(json!({
        "nodes": [{"id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "2000", "chain": "base"}}],
        "edges": []
    }))
    .unwrap();
    let trace = run(&SimulationInput {
        graph,
        config: config("USDC", "5000", 2),
        policy: amm_policy(),
        market_data: bundle(
            &[("2000", "2000", "2000", "2000"), ("2000", "2000", "2000", "2000")],
            Some(("100", "200000")),
        ),
    })
    .unwrap();
    let detail = event_detail(&trace, "action_executed");
    assert_eq!(num(detail, "price"), 2020.0, "market taker path keeps depth impact");
    assert_eq!(detail.get("amm_theoretical_price"), None);
    assert_eq!(detail.get("amm_impact_exceeds_limit"), None);
}
