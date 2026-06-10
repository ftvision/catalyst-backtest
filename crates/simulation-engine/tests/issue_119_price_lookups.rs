//! Issue #119 — price-lookup defects in the simulation engine. All five
//! sub-bugs are now resolved: (a)-(d) fixed, (e) closed as decided semantics.
//!
//!   * `issue_119_mark_price_venue_scoped` (sub-bug a, FIXED): `mark_price` now
//!     uses a VENUE-SCOPED close (`close_at`), so a holding on venue A is valued
//!     from venue A's own candles (carry-forward), never venue B's that happens
//!     to share the symbol.
//!
//!   * `issue_119_default_unbounded_carry_forward` /
//!     `issue_119_max_mark_staleness_expires_stale_mark` (sub-bug b, FIXED):
//!     `data.max_mark_staleness` bounds the carry-forward; the default stays
//!     unbounded (a conscious default — no silent result change).
//!
//!   * `issue_119_unpriced_spot_warned_not_silent` (sub-bug c, FIXED): an
//!     unpriced holding is still excluded from equity but now surfaces a
//!     `valuation_warning` event + run warning, deduped once per run.
//!
//!   * `issue_119_gap_sizing_resolves_fill_still_rejects_same_bar` /
//!     `issue_119_gap_sizing_defers_and_fills_next_open` /
//!     `issue_119_sizing_respects_staleness_bound` (sub-bug d, FIXED): sizing's
//!     unit price is now the same bounded venue-scoped mark equity uses
//!     (`MarketContext::mark_close`); filling stays gated on an exact bar.
//!
//!   * `issue_119_same_tick_stale_tick_equity` (sub-bug e, DECIDED): every
//!     same-tick action sizes off the tick-start equity snapshot; same-tick
//!     `pct_portfolio` actions intentionally do not compound against each
//!     other (see docs/logic/sizing.md).

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

/// ISSUE #119 (sub-bug a) — FIXED: a 1 ETH holding on `venueA` is valued at ts(1)
/// from venueA's OWN candles. venueA/ETH has no ts(1) bar, so the venue-scoped
/// `mark_price` carries forward venueA's last-known close (1000) — it never
/// borrows venueB's 3000 candle just because the symbol string matches.
#[test]
fn issue_119_mark_price_venue_scoped() {
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
    // ts0: venueA/ETH bar exists -> 1000.
    assert_eq!(trace.snapshots[0].equity_usd.to_string(), "1000");
    // FIXED #119(a): ts1 has no venueA/ETH bar, so the 1 ETH on venueA carries
    // forward venueA's OWN close (1000) — NOT venueB's 3000.
    assert_eq!(
        trace.snapshots[1].equity_usd.to_string(),
        "1000",
        "FIXED #119(a): venue-scoped mark carries venueA's 1000, never venueB's 3000"
    );
    assert_ne!(
        trace.snapshots[1].equity_usd.to_string(),
        "3000",
        "FIXED #119(a): the venue-blind 3000 must no longer appear"
    );
}

/// ISSUE #119 (sub-bug a) edge — both venues priced at the same tick: each ETH
/// holding is marked at its OWN venue's close, not a single shared price.
/// venueA/ETH @1000 + venueB/ETH @3000, holding 1 ETH on each -> equity 4000.
#[test]
fn issue_119_each_venue_marked_at_its_own_price() {
    let market: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(2),
        "candles": [
            {"venue": "venueA", "symbol": "ETH", "quote": "USD", "points": [pt(0, "1000"), pt(1, "1000")]},
            {"venue": "venueB", "symbol": "ETH", "quote": "USD", "points": [pt(0, "3000"), pt(1, "3000")]}
        ],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap();
    let config: BacktestConfig = serde_json::from_value(json!({
        "start": ts(0), "end": ts(2), "interval": "1h",
        "initial_portfolio": {"venueA": {"ETH": "1"}, "venueB": {"ETH": "1"}}
    }))
    .unwrap();
    let trace = run(&SimulationInput { graph: inert_graph(), config, policy: research_policy(), market_data: market }).unwrap();

    assert_eq!(trace.snapshots.len(), 2);
    // 1 ETH @1000 (venueA) + 1 ETH @3000 (venueB) = 4000, at both ticks.
    assert_eq!(trace.snapshots[0].equity_usd.to_string(), "4000");
    assert_eq!(trace.snapshots[1].equity_usd.to_string(), "4000");
}

// ---------------------------------------------------------------------------
// (b) mark carry-forward: unbounded by default, bounded by max_mark_staleness.
// ---------------------------------------------------------------------------

/// venueA/ETH has only a ts(0) bar (close 1000); a separate venueA/BTC series
/// drives ticks ts0..ts5. The bundle for the staleness tests below.
fn gappy_eth_market() -> MarketDataBundle {
    let btc_points: Vec<Value> = (0..=5).map(|i| pt(i, "50000")).collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(6),
        "candles": [
            {"venue": "venueA", "symbol": "ETH", "quote": "USD", "points": [pt(0, "1000")]},
            {"venue": "venueA", "symbol": "BTC", "quote": "USD", "points": btc_points}
        ],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

fn gappy_eth_config() -> BacktestConfig {
    serde_json::from_value(json!({
        "start": ts(0), "end": ts(6), "interval": "1h",
        "initial_portfolio": {"venueA": {"ETH": "1"}}
    }))
    .unwrap()
}

/// ISSUE #119 (sub-bug b) — the DEFAULT pinned: with no `max_mark_staleness`
/// the venue-scoped carry-forward is unbounded, so the frozen ts0 close 1000
/// marks the 1 ETH at every tick, even 5 bars past its last candle. This is a
/// conscious default (bounding marks changes results, so it is opt-in), not a
/// silent gap: the bound is available via `data.max_mark_staleness`.
#[test]
fn issue_119_default_unbounded_carry_forward() {
    let input = SimulationInput {
        graph: inert_graph(),
        config: gappy_eth_config(),
        policy: research_policy(),
        market_data: gappy_eth_market(),
    };
    let trace = run(&input).unwrap();

    assert_eq!(trace.snapshots.len(), 6, "ticks ts0..ts5 from the BTC series");
    for i in 0..6 {
        assert_eq!(
            trace.snapshots[i].equity_usd.to_string(),
            "1000",
            "default (unbounded): ETH carried forward at 1000 to tick {i}"
        );
    }
    // The holding is priced at every tick, so no valuation warning fires.
    assert_eq!(count(&trace, "valuation_warning"), 0, "default: nothing unpriced, no warning");
}

/// ISSUE #119 (sub-bug b) — FIXED: with `data.max_mark_staleness = "3h"` the
/// ts0 close may be carried forward only through ts3. At ts4 and ts5 the mark
/// has expired, so the ETH holding is EXCLUDED from equity (0) and the
/// exclusion is surfaced through the same loud (c) path: one deduped
/// `valuation_warning` event + run warning naming the holding.
#[test]
fn issue_119_max_mark_staleness_expires_stale_mark() {
    let policy: SimulationPolicy = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "research_v1",
        "data": {"max_mark_staleness": "3h"}
    }))
    .unwrap();

    let input = SimulationInput {
        graph: inert_graph(),
        config: gappy_eth_config(),
        policy,
        market_data: gappy_eth_market(),
    };
    let trace = run(&input).unwrap();

    assert_eq!(trace.snapshots.len(), 6, "ticks ts0..ts5 from the BTC series");
    // Within the 3h bound the ts0 close still marks the holding...
    for i in 0..4 {
        assert_eq!(
            trace.snapshots[i].equity_usd.to_string(),
            "1000",
            "FIXED #119(b): mark within the 3h bound at tick {i}"
        );
    }
    // ...beyond it the mark expires and the holding is excluded, loudly.
    for i in 4..6 {
        assert_eq!(
            trace.snapshots[i].equity_usd.to_string(),
            "0",
            "FIXED #119(b): stale mark expired at tick {i}; holding excluded"
        );
    }
    assert_eq!(
        count(&trace, "valuation_warning"),
        1,
        "the expired mark is surfaced exactly once per run (dedup)"
    );
    assert!(
        trace.warnings.iter().any(|w| w.contains("venueA/ETH") && w.contains("excluded")),
        "run warning names the excluded holding; got {:?}",
        trace.warnings
    );
}

// ---------------------------------------------------------------------------
// (c) an unpriced non-stable holding is excluded from equity — loudly.
// ---------------------------------------------------------------------------

/// ISSUE #119 (sub-bug c) — FIXED: venueA holds 2 WBTC + 500 USDC, but NO WBTC
/// candle exists anywhere, so the WBTC mark is None. The WBTC is still
/// EXCLUDED from equity (only the 500 USDC counts — unchanged math), but the
/// exclusion is no longer silent: a `valuation_warning` event and a run
/// warning name the holding, deduped to exactly once per run even though the
/// holding is unpriced at every tick.
#[test]
fn issue_119_unpriced_spot_warned_not_silent() {
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
    // Equity math unchanged: the 2 WBTC are excluded; only the 500 USDC counts.
    assert_eq!(
        trace.snapshots[0].equity_usd.to_string(),
        "500",
        "FIXED #119(c): unpriced WBTC excluded; equity is the 500 USDC only"
    );
    assert_eq!(
        trace.snapshots[1].equity_usd.to_string(),
        "500",
        "FIXED #119(c): WBTC still excluded at ts1"
    );
    // ...but the exclusion is LOUD: the run warning names the holding.
    assert!(
        trace.warnings.iter().any(|w| w.contains("venueA/WBTC") && w.contains("excluded")),
        "FIXED #119(c): a warning must name the unpriced WBTC holding; got {:?}",
        trace.warnings
    );
    // Dedup pin: the holding is unpriced at BOTH ticks but warned exactly once.
    assert_eq!(
        count(&trace, "valuation_warning"),
        1,
        "exactly one valuation_warning across the multi-tick run (dedup per run)"
    );
    let ev = trace.events.iter().find(|e| e.event_type == "valuation_warning").unwrap();
    let detail = ev.detail.as_ref().unwrap();
    assert_eq!(detail["venue"], json!("venueA"));
    assert_eq!(detail["asset"], json!("WBTC"));
    assert_eq!(detail["kind"], json!("spot"));
    assert_eq!(detail["amount"], json!("2"));
}

/// INTENDED SEMANTICS of the (a) fix, pinned: a holding on a venue with NO
/// candles of its own is EXCLUDED from equity even when another venue prices
/// the same symbol. Pre-fix, the venue-blind `price_any` fallback would borrow
/// venueA's 1000 and count the venueB ETH at $1000; post-fix venueB's ETH
/// contributes 0 — and with (c) fixed, the exclusion is surfaced loudly.
///
/// Note the same window cannot arise for a *yield* position: a non-stable
/// yield deposit is rejected without an exact bar on its chain (#115), and the
/// venue-scoped carry-forward keeps it priced afterwards.
#[test]
fn issue_119_cross_venue_holding_excluded_not_borrowed() {
    let market: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(2),
        "candles": [
            // ETH is priced ONLY on venueA. venueB has no candles at all.
            {"venue": "venueA", "symbol": "ETH", "quote": "USD",
             "points": [pt(0, "1000"), pt(1, "1000")]}
        ],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap();

    let config: BacktestConfig = serde_json::from_value(json!({
        "start": ts(0), "end": ts(2), "interval": "1h",
        "initial_portfolio": {"venueB": {"ETH": "1", "USDC": "500"}}
    }))
    .unwrap();

    let input = SimulationInput { graph: inert_graph(), config, policy: research_policy(), market_data: market };
    let trace = run(&input).unwrap();

    assert_eq!(trace.snapshots.len(), 2);
    for (i, snap) in trace.snapshots.iter().enumerate() {
        assert_eq!(
            snap.equity_usd.to_string(),
            "500",
            "#119(a) semantics: venueB's ETH must NOT borrow venueA's price; \
             equity at ts{i} is the 500 USDC only"
        );
    }
    // FIXED #119(c): the cross-venue exclusion is surfaced, not silent.
    assert_eq!(count(&trace, "valuation_warning"), 1, "exclusion warned exactly once");
    assert!(
        trace.warnings.iter().any(|w| w.contains("venueB/ETH") && w.contains("excluded")),
        "run warning names the excluded venueB/ETH holding; got {:?}",
        trace.warnings
    );
}

/// ISSUE #119 (sub-bug c, perp leg) — FIXED: a perp whose venue has no mark
/// keeps its MARGIN in equity (posted cash is real) but its unrealized PnL is
/// excluded — and that exclusion now fires a `valuation_warning` with kind
/// "perp_pnl". The mark is expired via the (b) staleness bound, since the perp
/// could only have been opened against an existing bar.
#[test]
fn issue_119_unpriced_perp_pnl_warned_margin_counted() {
    let market: MarketDataBundle = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(3),
        "candles": [
            // ETH has only the ts0 bar (the perp opens against it)...
            {"venue": "hyperliquid", "symbol": "ETH", "quote": "USD", "points": [pt(0, "2000")]},
            // ...while BTC drives ticks ts0..ts2.
            {"venue": "hyperliquid", "symbol": "BTC", "quote": "USD",
             "points": [pt(0, "50000"), pt(1, "50000"), pt(2, "50000")]}
        ],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap();

    let config: BacktestConfig = serde_json::from_value(json!({
        "start": ts(0), "end": ts(3), "interval": "1h",
        "initial_portfolio": {"hyperliquid": {"USDC": "2000"}}
    }))
    .unwrap();

    // research_v1 (same-bar close fill) with costs zeroed so equity stays a
    // round 2000, gas off, and a 1h staleness bound: the ts0 ETH mark is alive
    // at ts1 (ts1 - 1h = ts0) but expired at ts2.
    let policy: SimulationPolicy = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "research_v1",
        "fills": {"slippage": {"model": "none"}, "fees": {"model": "none"}},
        "gas": {"model": "none"},
        "data": {"max_mark_staleness": "1h"}
    }))
    .unwrap();

    // A single initial action: long 500 USD of ETH on the first tick.
    let graph: Graph = serde_json::from_value(json!({
        "nodes": [
            {"id": "open", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "long", "size_usd": "500", "chain": "hyperliquid"}}
        ],
        "edges": []
    }))
    .unwrap();

    let trace = run(&SimulationInput { graph, config, policy, market_data: market }).unwrap();
    assert_eq!(count(&trace, "action_executed"), 1, "the perp opens at ts0");
    assert_eq!(trace.snapshots.len(), 3);

    // ts0/ts1: mark alive (exact bar, then 1h carry) -> cash 1500 + margin 500
    // + PnL 0 = 2000. ts2: mark expired -> PnL excluded but the margin still
    // counts, so equity stays 2000 (NOT 1500: margin is never dropped).
    for (i, snap) in trace.snapshots.iter().enumerate() {
        assert_eq!(snap.equity_usd.to_string(), "2000", "equity at ts{i}");
    }

    assert_eq!(count(&trace, "valuation_warning"), 1, "the unmarked perp warns exactly once");
    let ev = trace.events.iter().find(|e| e.event_type == "valuation_warning").unwrap();
    let detail = ev.detail.as_ref().unwrap();
    assert_eq!(detail["venue"], json!("hyperliquid"));
    assert_eq!(detail["asset"], json!("ETH"));
    assert_eq!(detail["kind"], json!("perp_pnl"));
    assert!(
        trace.warnings.iter().any(|w| w.contains("perp hyperliquid/ETH")),
        "run warning names the unmarked perp; got {:?}",
        trace.warnings
    );
}

// ---------------------------------------------------------------------------
// (d) pct_portfolio sizing prices off the same bounded venue-scoped mark as
//     equity; filling stays gated on an exact bar.
// ---------------------------------------------------------------------------

/// research_v1 with a `level` signal trigger so a threshold signal fires every
/// tick its condition holds.
fn level_research_policy() -> SimulationPolicy {
    let mut p = research_policy();
    p.signals = Some(SignalPolicy { trigger: Some("level".into()), repeat: None, cooldown: None, max_count: None });
    p
}

/// venueA/ETH has a ts0 bar (2000) then a gap at ts1; venueA/BTC drives the
/// ticks; a funding print at ts1 fires the signal on the gap bar. Extra ETH
/// points (e.g. a ts2 bar for the next_open test) can be appended.
fn gap_sizing_market(extra_eth_points: Vec<Value>, end_tick: i64) -> MarketDataBundle {
    let mut eth_points = vec![pt(0, "2000")];
    eth_points.extend(extra_eth_points);
    let btc_points: Vec<Value> = (0..end_tick).map(|i| pt(i, "50000")).collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(end_tick),
        "candles": [
            {"venue": "venueA", "symbol": "ETH", "quote": "USD", "points": eth_points},
            {"venue": "venueA", "symbol": "BTC", "quote": "USD", "points": btc_points}
        ],
        "funding": [
            {"venue": "venueA", "symbol": "ETH", "points": [{"ts": ts(1), "rate": "0.0002"}]}
        ],
        "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

fn gap_sizing_config(end_tick: i64) -> BacktestConfig {
    serde_json::from_value(json!({
        "start": ts(0), "end": ts(end_tick), "interval": "1h",
        "initial_portfolio": {"venueA": {"ETH": "1"}}
    }))
    .unwrap()
}

/// funding(venueA/ETH) >= 0.0001 -> sell ETH sized pct_portfolio 10%.
fn gap_sizing_graph() -> Graph {
    serde_json::from_value(json!({
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
    .unwrap()
}

/// ISSUE #119 (sub-bug d) — FIXED, same-bar half: at ts1 the funding signal
/// fires and tries to SELL ETH sized `pct_portfolio: 10` on the gap bar.
/// Sizing now resolves: `asset_price` reads `MarketContext::mark_close` — the
/// same venue-scoped carry-forward equity uses — so the unit price is 2000 and
/// the 10% slice resolves to 0.1 ETH (no more sizing/equity mismatch). The
/// FILL still rejects, correctly: under research_v1's same-bar close selection
/// `execute_swap` needs an exact bar to trade against ("no price" guard) and
/// ts1 has none. You can size with a mark; you can't fill at one.
#[test]
fn issue_119_gap_sizing_resolves_fill_still_rejects_same_bar() {
    let input = SimulationInput {
        graph: gap_sizing_graph(),
        config: gap_sizing_config(2),
        policy: level_research_policy(),
        market_data: gap_sizing_market(vec![], 2),
    };
    let trace = run(&input).unwrap();

    // Equity at ts1 is priced via the carry-forward (2000) — and now sizing
    // reads the same mark.
    assert_eq!(trace.snapshots.len(), 2);
    assert_eq!(
        trace.snapshots[1].equity_usd.to_string(),
        "2000",
        "equity carries ETH forward to 2000 at ts1; sizing reads the same mark"
    );

    // FIXED #119(d): the rejection is no longer the sizing guard...
    assert!(
        !trace.events.iter().any(|e| {
            e.reason.as_deref() == Some("pct_portfolio sizing needs a price for the action asset")
        }),
        "FIXED #119(d): sizing must resolve at the carried mark, not reject"
    );
    // ...it is the execution model's exact-bar fill gate: a same-bar market
    // order cannot trade on a bar that doesn't exist.
    assert_eq!(count(&trace, "action_executed"), 0, "no fill on the gap bar");
    assert_eq!(count(&trace, "action_rejected"), 1, "exactly one rejection");
    assert!(
        trace.events.iter().any(|e| {
            e.event_type == "action_rejected"
                && e.reason.as_deref() == Some("no price for ETH on venueA")
        }),
        "the rejection is the fill's exact-bar gate, not the sizing guard; got {:?}",
        trace.events.iter().filter(|e| e.event_type == "action_rejected").collect::<Vec<_>>()
    );
}

/// ISSUE #119 (sub-bug d) — FIXED, next_open half (the real improvement, not
/// just consistency): under strict_v1's `next_open` selection the same gap-bar
/// signal can now SIZE at the carried mark (0.1 ETH = 10% of 2000 / 2000),
/// DEFER the market order (#116), and legitimately FILL at the next bar's
/// open (ts2's 2200). Pre-fix the action died in sizing on the gap bar and the
/// strategy missed a tradeable opportunity.
#[test]
fn issue_119_gap_sizing_defers_and_fills_next_open() {
    // strict_v1 (next_open) with the interior-gap abort relaxed to a warning
    // (the gap IS the scenario) and costs zeroed for round numbers.
    let policy: SimulationPolicy = serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "signals": {"trigger": "level"},
        "fills": {"slippage": {"model": "none"}, "fees": {"model": "none"}},
        "gas": {"model": "none"},
        "data": {"missing_required": "warn"}
    }))
    .unwrap();

    // ETH returns at ts2 with a 2200 bar — the deferred order's next open.
    let input = SimulationInput {
        graph: gap_sizing_graph(),
        config: gap_sizing_config(3),
        policy,
        market_data: gap_sizing_market(vec![pt(2, "2200")], 3),
    };
    let trace = run(&input).unwrap();

    // ts1 (gap bar): sizing resolves at the carried mark and the market order
    // is deferred — no rejection, no same-bar fill.
    assert_eq!(count(&trace, "action_rejected"), 0, "FIXED #119(d): nothing rejected");
    assert_eq!(count(&trace, "order_deferred"), 1, "the sized order defers on the gap bar");
    let deferred = trace.events.iter().find(|e| e.event_type == "order_deferred").unwrap();
    assert_eq!(deferred.ts, ts(1), "deferred on the decision (gap) bar");

    // ts2: the deferred order fills at the next bar's open (2200), booked there.
    assert_eq!(count(&trace, "action_executed"), 1, "exactly one fill");
    let exec = trace.events.iter().find(|e| e.event_type == "action_executed").unwrap();
    assert_eq!(exec.ts, ts(2), "booked on the fill bar (#116), not the decision bar");
    let detail = exec.detail.as_ref().unwrap();
    assert_eq!(detail["price"], json!("2200"), "filled at ts2's open");
    // Sized at the carried mark: 10% of 2000 equity / 2000 unit price = 0.1 ETH.
    assert_eq!(detail["amount"], json!("0.1"), "sized at the ts1 carried mark (2000)");

    // Ledger: 0.9 ETH + 220 USDC proceeds; ts2 equity = 0.9*2200 + 220 = 2200.
    assert_eq!(trace.snapshots.len(), 3);
    assert_eq!(trace.snapshots[2].equity_usd.to_string(), "2200");
}

/// ISSUE #119 (sub-bug d) — the unified sizing price respects the (b)
/// staleness bound: with `data.max_mark_staleness = "30m"` the ts0 close has
/// expired by ts1 (1h later), so the mark is None, `asset_price` returns 0,
/// and sizing rejects through the existing zero-price guard. Sizing and
/// equity agree here too: the holding is simultaneously excluded from equity
/// (the (b)/(c) path).
#[test]
fn issue_119_sizing_respects_staleness_bound() {
    let mut policy = level_research_policy();
    policy.data = Some(serde_json::from_value(json!({"max_mark_staleness": "30m"})).unwrap());

    let input = SimulationInput {
        graph: gap_sizing_graph(),
        config: gap_sizing_config(2),
        policy,
        market_data: gap_sizing_market(vec![], 2),
    };
    let trace = run(&input).unwrap();

    // The expired mark drops the holding from equity AND from sizing — the two
    // never disagree about whether the asset is priced.
    assert_eq!(trace.snapshots.len(), 2);
    assert_eq!(
        trace.snapshots[1].equity_usd.to_string(),
        "0",
        "the 30m bound expires the ts0 mark at ts1; the holding is excluded"
    );
    assert_eq!(count(&trace, "action_executed"), 0);
    assert_eq!(count(&trace, "action_rejected"), 1, "sizing rejects on the expired mark");
    assert!(
        trace.events.iter().any(|e| {
            e.event_type == "action_rejected"
                && e.reason.as_deref() == Some("pct_portfolio sizing needs a price for the action asset")
        }),
        "the zero-price sizing guard fires when the mark is expired"
    );
}

// ---------------------------------------------------------------------------
// (e) tick_equity is a tick-start snapshot shared by all same-tick actions —
//     DECIDED semantics, pinned here (see docs/logic/sizing.md).
// ---------------------------------------------------------------------------

/// ISSUE #119 (sub-bug e) — DECIDED SEMANTICS, pinned: every same-tick action
/// sizes off the tick-start equity snapshot; same-tick `pct_portfolio` actions
/// do NOT compound against each other. Two perp_order actions run in the SAME
/// first tick, a (long ETH, 25% portfolio) then b (long BTC, 25% portfolio);
/// `tick_equity` is computed once at tick start (2000) and passed unchanged to
/// BOTH, so both report `value_usd == "500"` — b does not see a's fee/entry
/// impact (a recompute engine would size b at ~499.875).
///
/// Why this is the decided behavior rather than a bug:
///   * Under strict/next_open profiles it is a no-op — deferred fills don't
///     touch the ledger on the decision tick, so there is nothing fresher to
///     read; only same-bar (research) profiles see any difference at all.
///   * That residual difference is fee-sized (~0.025% here), not
///     direction-changing.
///   * Recomputing equity between same-tick actions would make sizing depend
///     on intra-tick action order — exactly the dimension the unimplemented
///     `ordering.same_tick` variants (#141) are supposed to govern. If a
///     recompute is ever wanted, it should arrive as an explicit, wired sizing
///     knob alongside #141, not as a silent default change.
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
    // DECIDED #119(e): action b shares the SAME tick-start snapshot -> 500 too.
    // Same-tick pct_portfolio actions do not compound; a recompute-between-
    // actions engine would give ~499.875 (and intra-tick order-sensitivity,
    // which belongs to #141's ordering knobs, not to a silent default).
    assert_eq!(
        b_val,
        json!("500"),
        "DECIDED #119(e): action b sizes off the shared tick-start equity (2000)"
    );
}
