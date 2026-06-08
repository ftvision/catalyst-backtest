//! Issue #119 — price-lookup defects in the simulation engine.
//!
//! All tests below record bugs that are STILL PRESENT in the current engine.
//! Each PINS the current (incorrect) value so it PASSES today, and documents
//! the CORRECT value a fixed engine should produce.
//!
//!   * `issue_119_mark_price_venue_blind` (sub-bug a): `mark_price` falls back
//!     to a venue-blind `price_any`, so a holding on venue A is valued using
//!     venue B's candle when both share the symbol string.
//!
//!   * `issue_119_price_any_unbounded_staleness` (sub-bug b): `price_any`
//!     carries forward a stale last-known price across an arbitrary gap with no
//!     staleness bound and no warning.
//!
//!   * `issue_119_unpriced_spot_silently_dropped` (sub-bug c): `compute_equity`
//!     silently drops a non-stable spot holding (contributes 0, no warning)
//!     when it has no mark.
//!
//!   * `issue_119_pct_portfolio_rejected_on_gap` (sub-bug d): `pct_portfolio`
//!     sizing is rejected on a gap bar because `asset_price` uses an exact
//!     `bar_at` -> 0 -> the `unit_price.is_zero()` guard, even though equity's
//!     `mark_price` could still price the asset (carry-forward).
//!
//!   * `issue_119_same_tick_stale_tick_equity` (sub-bug e): `tick_equity` is a
//!     tick-start snapshot reused for all of a tick's actions, so a 2nd
//!     same-tick action's `pct_portfolio` sizes off pre-1st-action equity.
//!
//! Sub-bug (c)-perp (drop a perp's unrealized PnL when mark missing) is NOT
//! tested: opening a perp populates the symbol price, so `price_any` always
//! returns a carried-forward mark and the `else { margin only }` branch is dead
//! under the current fallback. See the issue notes.

use catalyst_contracts::policy::SignalPolicy;
use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START_EPOCH: i64 = 1_704_067_200;
const STEP: i64 = 3600;

fn ts(i: i64) -> String {
    let epoch = START_EPOCH + i * STEP;
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A flat candle point (open=high=low=close=c) at tick `i`.
fn pt(i: i64, c: &str) -> Value {
    json!({"ts": ts(i), "open": c, "high": c, "low": c, "close": c})
}

/// research_v1: missing_required=forward_fill so interior gaps WARN, not abort.
fn research_policy() -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "research_v1"
    }))
    .unwrap()
}

/// An inert graph: a price_threshold signal that NEVER fires (threshold absurdly
/// high) wired to a dummy swap. A graph needs >=1 node to compile; this one
/// never touches the holding under test.
fn inert_graph() -> Graph {
    serde_json::from_value(json!({
        "nodes": [
            {"id": "never", "kind": "signal", "subtype": "price_threshold",
             "config": {"symbol": "ETH", "operator": ">", "threshold": "99999999"}},
            {"id": "noop", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "1", "chain": "venueA"}}
        ],
        "edges": [{"from": "never", "to": "noop"}]
    }))
    .unwrap()
}

fn count(t: &catalyst_contracts::SimulationTrace, k: &str) -> usize {
    t.events.iter().filter(|e| e.event_type == k).count()
}

// ---------------------------------------------------------------------------
// (a) mark_price is venue-blind: venueA/ETH priced off venueB/ETH candle.
// ---------------------------------------------------------------------------

/// ISSUE #119 (sub-bug a): a 1 ETH holding on `venueA` is valued at ts(1) using
/// `venueB`'s ETH candle (3000) because `venueA/ETH` has no ts(1) bar and
/// `mark_price` falls back to the venue-blind `price_any("ETH", ts1)`.
///
/// The engine reports `snapshots[1].equity_usd == "3000"`, which is INCORRECT.
/// The CORRECT value is "1000": venue-scoped pricing should carry forward
/// venueA's own last-known close (1000), never borrow venueB's 3000.
#[test]
fn issue_119_mark_price_venue_blind() {
    let market: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(2),
        "candles": [
            // venueA/ETH only has a ts0 bar (close 1000) -> gap at ts1.
            {"venue": "venueA", "symbol": "ETH", "quote": "USD", "points": [pt(0, "1000")]},
            // venueB/ETH has ts0 AND ts1 bars (close 3000) -> creates the ts1 tick.
            {"venue": "venueB", "symbol": "ETH", "quote": "USD",
             "points": [pt(0, "3000"), pt(1, "3000")]}
        ],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap();

    let config: BacktestConfig = serde_json::from_value(json!({
        "start": ts(0), "end": ts(2), "interval": "1h",
        "initial_portfolio": {"venueA": {"ETH": "1"}}
    }))
    .unwrap();

    let input = SimulationInput { graph: inert_graph(), config, policy: research_policy(), market_data: market };
    let trace = run(&input).unwrap();

    assert_eq!(trace.snapshots.len(), 2);
    // ts0: venueA/ETH bar exists -> correctly 1000.
    assert_eq!(trace.snapshots[0].equity_usd.to_string(), "1000");
    // PIN THE BUG: ts1 has no venueA/ETH bar, so the 1 ETH on venueA is priced
    // at venueB's 3000. CORRECT would be "1000" (carry forward venueA's close).
    assert_eq!(
        trace.snapshots[1].equity_usd.to_string(),
        "3000",
        "BUG #119(a): venueA ETH valued at venueB's candle (venue-blind price_any); correct is 1000"
    );
    assert_ne!(
        trace.snapshots[1].equity_usd.to_string(),
        "1000",
        "BUG #119(a) witness: a venue-scoped fix would give 1000, never venueB's 3000"
    );
}

// ---------------------------------------------------------------------------
// (b) price_any carries a stale price forever with no staleness bound.
// ---------------------------------------------------------------------------

/// ISSUE #119 (sub-bug b): venueA/ETH has only a ts(0) bar (close 1000). A
/// separate venueA/BTC series drives ticks ts0..ts5. For every tick after ts0
/// the ETH mark falls to `price_any`, which returns the frozen ts0 close 1000
/// with NO upper time bound and NO staleness warning.
///
/// The engine reports `equity_usd == "1000"` at EVERY tick, including ts5 (5
/// bars after the last ETH candle), which is INCORRECT. A fixed engine with a
/// finite staleness bound (< 5 bars) would stop returning 1000 and/or emit a
/// staleness warning rather than silently holding 1000 forever.
#[test]
fn issue_119_price_any_unbounded_staleness() {
    let btc_points: Vec<Value> = (0..=5).map(|i| pt(i, "50000")).collect();
    let market: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(6),
        "candles": [
            {"venue": "venueA", "symbol": "ETH", "quote": "USD", "points": [pt(0, "1000")]},
            {"venue": "venueA", "symbol": "BTC", "quote": "USD", "points": btc_points}
        ],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap();

    let config: BacktestConfig = serde_json::from_value(json!({
        "start": ts(0), "end": ts(6), "interval": "1h",
        "initial_portfolio": {"venueA": {"ETH": "1"}}
    }))
    .unwrap();

    let input = SimulationInput { graph: inert_graph(), config, policy: research_policy(), market_data: market };
    let trace = run(&input).unwrap();

    assert_eq!(trace.snapshots.len(), 6, "ticks ts0..ts5 from the BTC series");
    // PIN THE BUG: the 1 ETH is held at the frozen 1000 at every tick, even 5
    // bars past its last candle. CORRECT: a finite staleness bound would make
    // the price None (so equity != 1000) and/or surface a staleness warning.
    for i in 0..6 {
        assert_eq!(
            trace.snapshots[i].equity_usd.to_string(),
            "1000",
            "BUG #119(b): stale ETH price carried forward unbounded to tick {i}"
        );
    }
    // No staleness warning is emitted (part of the bug: the carry is silent).
    assert!(
        !trace.warnings.iter().any(|w| {
            let lw = w.to_lowercase();
            lw.contains("stale") || lw.contains("staleness")
        }),
        "BUG #119(b): no staleness warning emitted for a 5-bar-old carried-forward price"
    );
}

// ---------------------------------------------------------------------------
// (c) compute_equity silently drops an unpriced non-stable spot holding.
// ---------------------------------------------------------------------------

/// ISSUE #119 (sub-bug c): venueA holds 2 WBTC + 500 USDC, but NO WBTC candle
/// exists on any venue, so `mark_price`/`price_any` for WBTC is None. In
/// `compute_equity` the non-stable WBTC takes the `else if let Some(price)`
/// arm, which is skipped -> it contributes 0. Only the 500 USDC counts.
///
/// The engine reports `equity_usd == "500"` at every tick, which is INCORRECT
/// (the 2 WBTC are silently zeroed). At minimum a fixed engine should emit a
/// warning about the unpriced holding rather than silently understate equity.
#[test]
fn issue_119_unpriced_spot_silently_dropped() {
    let market: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(2),
        "candles": [
            // Unrelated ETH series only drives the ticks; no WBTC series anywhere.
            {"venue": "venueA", "symbol": "ETH", "quote": "USD",
             "points": [pt(0, "1000"), pt(1, "1000")]}
        ],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap();

    let config: BacktestConfig = serde_json::from_value(json!({
        "start": ts(0), "end": ts(2), "interval": "1h",
        "initial_portfolio": {"venueA": {"WBTC": "2", "USDC": "500"}}
    }))
    .unwrap();

    let input = SimulationInput { graph: inert_graph(), config, policy: research_policy(), market_data: market };
    let trace = run(&input).unwrap();

    assert_eq!(trace.snapshots.len(), 2);
    // PIN THE BUG: the 2 WBTC contribute 0; only the 500 USDC is counted.
    assert_eq!(
        trace.snapshots[0].equity_usd.to_string(),
        "500",
        "BUG #119(c): unpriced WBTC silently dropped; equity is the 500 USDC only"
    );
    assert_eq!(
        trace.snapshots[1].equity_usd.to_string(),
        "500",
        "BUG #119(c): WBTC still dropped at ts1"
    );
    // No warning surfaces the dropped holding (part of the bug).
    assert!(
        !trace.warnings.iter().any(|w| {
            let lw = w.to_lowercase();
            lw.contains("wbtc") || lw.contains("unpriced")
        }),
        "BUG #119(c): holding silently dropped with no warning; a fix should warn or not understate"
    );
}

// ---------------------------------------------------------------------------
// (d) pct_portfolio sizing rejected on a gap bar even though equity prices it.
// ---------------------------------------------------------------------------

/// research_v1 with a `level` signal trigger so a threshold signal fires every
/// tick its condition holds.
fn level_research_policy() -> SimulationPolicy {
    let mut p = research_policy();
    p.signals = Some(SignalPolicy { trigger: Some("level".into()), repeat: None, cooldown: None, max_count: None });
    p
}

/// ISSUE #119 (sub-bug d): at ts1 the funding signal fires and tries to SELL ETH
/// sized `pct_portfolio: 10`. `compute_equity` prices the 1 ETH via
/// `mark_price` -> `price_any` carry-forward (2000), so equity at ts1 is 2000.
/// But `execute_action`'s `asset_price(venueA, ETH)` uses the EXACT `bar_at`
/// which is None at ts1 -> 0 -> `resolve_amount` hits the `unit_price.is_zero()`
/// guard and REJECTS.
///
/// The engine produces 1 `action_rejected` ("pct_portfolio sizing needs a price
/// for the action asset") and 0 `action_executed`, which is INCORRECT. Sizing
/// should use the same priced lookup as equity (carry-forward 2000), sizing the
/// sell at 10% of 2000 = $200 = 0.1 ETH and EXECUTING it (1 executed, 0
/// rejected).
#[test]
fn issue_119_pct_portfolio_rejected_on_gap() {
    let market: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(2),
        "candles": [
            // venueA/ETH only has a ts0 bar (2000) -> gap at ts1.
            {"venue": "venueA", "symbol": "ETH", "quote": "USD", "points": [pt(0, "2000")]},
            // venueA/BTC drives ticks ts0 and ts1.
            {"venue": "venueA", "symbol": "BTC", "quote": "USD",
             "points": [pt(0, "50000"), pt(1, "50000")]}
        ],
        "funding": [
            {"venue": "venueA", "symbol": "ETH", "points": [{"ts": ts(1), "rate": "0.0002"}]}
        ],
        "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap();

    let config: BacktestConfig = serde_json::from_value(json!({
        "start": ts(0), "end": ts(2), "interval": "1h",
        "initial_portfolio": {"venueA": {"ETH": "1"}}
    }))
    .unwrap();

    // funding(venueA/ETH) >= 0.0001 -> sell ETH sized pct_portfolio 10%.
    let graph: Graph = serde_json::from_value(json!({
        "nodes": [
            {"id": "fund", "kind": "signal", "subtype": "threshold",
             "config": {
                 "source": {"kind": "funding", "venue": "venueA", "symbol": "ETH"},
                 "operator": ">=",
                 "reference": {"const": "0.0001"}
             }},
            {"id": "sell", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "ETH", "to_asset": "USDC",
                        "amount": {"basis": "pct_portfolio", "value": "10"}, "chain": "venueA"}}
        ],
        "edges": [{"from": "fund", "to": "sell"}]
    }))
    .unwrap();

    let input = SimulationInput { graph, config, policy: level_research_policy(), market_data: market };
    let trace = run(&input).unwrap();

    // Equity at ts1 IS priced (carry-forward 2000), proving sizing COULD price it.
    assert_eq!(trace.snapshots.len(), 2);
    assert_eq!(
        trace.snapshots[1].equity_usd.to_string(),
        "2000",
        "equity carries ETH forward to 2000 at ts1, so sizing could too"
    );

    // PIN THE BUG: the sell is rejected for lack of a price even though equity
    // priced the asset. CORRECT: 1 executed, 0 rejected.
    assert_eq!(
        count(&trace, "action_executed"),
        0,
        "BUG #119(d): sell rejected on the gap bar; correct behavior executes 1"
    );
    assert_eq!(
        count(&trace, "action_rejected"),
        1,
        "BUG #119(d): exactly one rejection on the gap bar"
    );
    assert!(
        trace.events.iter().any(|e| {
            e.event_type == "action_rejected"
                && e.reason.as_deref() == Some("pct_portfolio sizing needs a price for the action asset")
        }),
        "BUG #119(d): rejection cites the missing exact-bar price even though mark_price has one"
    );
}

// ---------------------------------------------------------------------------
// (e) tick_equity is a tick-start snapshot reused for all same-tick actions.
// ---------------------------------------------------------------------------

/// ISSUE #119 (sub-bug e): two perp_order actions run in the SAME first tick, a
/// (long ETH, 25% portfolio) then b (long BTC, 25% portfolio). `tick_equity` is
/// computed once at tick start (2000) and passed unchanged to BOTH actions, so
/// action b sizes off pre-action-a equity.
///
/// The engine reports both `value_usd == "500"` (25% of the stale 2000),
/// which is INCORRECT for b: action a pays a fee + opens above mark, dropping
/// equity below 2000, so a recompute-between-actions engine would size b at 25%
/// of the post-a equity (~1999.50) = ~499.875, i.e. strictly less than 500.
#[test]
fn issue_119_same_tick_stale_tick_equity() {
    let market: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(2),
        "candles": [
            {"venue": "hyperliquid", "symbol": "ETH", "quote": "USD",
             "points": [pt(0, "2000"), pt(1, "2000")]},
            {"venue": "hyperliquid", "symbol": "BTC", "quote": "USD",
             "points": [pt(0, "40000"), pt(1, "40000")]}
        ],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap();

    let config: BacktestConfig = serde_json::from_value(json!({
        "start": ts(0), "end": ts(2), "interval": "1h",
        "initial_portfolio": {"hyperliquid": {"USDC": "2000"}}
    }))
    .unwrap();

    // Two initial perp_orders chained a -> b; both run in the first tick.
    let graph: Graph = serde_json::from_value(json!({
        "nodes": [
            {"id": "a", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "long",
                        "size_usd": {"basis": "pct_portfolio", "value": "25"}, "chain": "hyperliquid"}},
            {"id": "b", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "BTC", "side": "long",
                        "size_usd": {"basis": "pct_portfolio", "value": "25"}, "chain": "hyperliquid"}}
        ],
        "edges": [{"from": "a", "to": "b"}]
    }))
    .unwrap();

    let input = SimulationInput { graph, config, policy: research_policy(), market_data: market };
    let trace = run(&input).unwrap();

    assert_eq!(count(&trace, "action_executed"), 2, "both perp orders execute");
    assert_eq!(count(&trace, "action_rejected"), 0);

    let execs: Vec<_> =
        trace.events.iter().filter(|e| e.event_type == "action_executed").collect();
    let a = execs.iter().find(|e| e.node_id.as_deref() == Some("a")).expect("action a executed");
    let b = execs.iter().find(|e| e.node_id.as_deref() == Some("b")).expect("action b executed");

    let a_val = a.detail.as_ref().unwrap()["value_usd"].clone();
    let b_val = b.detail.as_ref().unwrap()["value_usd"].clone();

    // Action a is sized off the tick-start equity 2000 -> 25% = 500.
    assert_eq!(a_val, json!("500"), "action a: 25% of tick-start equity 2000");
    // PIN THE BUG: action b reuses the STALE tick-start equity 2000 -> 500 again.
    // CORRECT (recompute between same-tick actions): b would be 25% of post-a
    // equity (~1999.50) = ~499.875, strictly less than 500.
    assert_eq!(
        b_val,
        json!("500"),
        "BUG #119(e): action b sized off stale tick-start equity (2000); a recompute would give ~499.875"
    );
}
