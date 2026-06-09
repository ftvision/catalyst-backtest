//! Issue #116 (FIXED) — market-only `next_open` deferral, EDGE CASES not already
//! covered by `issue_116_next_open_booking.rs` (which covers next-tick 1h/4h and
//! end-of-horizon drop).
//!
//! Spec recap (strict_v1 / next_open):
//! A MARKET swap/perp decided on bar N fills at bar N+1's OPEN and is BOOKED at
//! bar N+1 (not bar N). Consequences: the position appears in the snapshot at the
//! FILL bar; the `action_executed` event is timestamped at the fill bar; funding/
//! yield on that position start at the fill bar; a MARKET order on the FINAL bar
//! (no next bar) is DROPPED. UNCHANGED: limit placement and yield deposit/withdraw
//! still execute on the decision bar; the fill PRICE is still the next bar's open.
//! Chained market actions take one bar per hop: a downstream market order is
//! DECIDED on its parent's fill bar, so it defers again and fills one bar later.
//!
//! Tests here (each fn documents its own scenario):
//! - `gap_market_fill_lands_on_next_actual_bar_open_and_funding_spans_gap`: a data
//!   gap between the decision bar and the next available bar — the fill lands on the
//!   next actual bar's open and funding spans the full elapsed gap.
//! - `market_then_market_downstream_defers_one_more_bar`: a market action chained
//!   after another is decided on the parent's fill bar and fills one bar later
//!   (one bar per hop, the same next_open rule applied uniformly).
//! - `yield_deposit_executes_decision_bar_then_market_fills_next_bar`: a yield
//!   deposit executes on the decision bar; the chained market fills the next bar.
//! - `two_signals_same_bar_both_market_orders_fill_next_bar`: two signals on one bar
//!   each trigger a market order; both defer and fill on the next bar.
//! - `sibling_limit_rests_from_decision_bar_while_market_defers`: a limit order rests
//!   from the decision bar (unchanged) while a sibling market order defers.
//! - `deferred_perp_not_funded_for_bar_before_its_fill`: the deferred perp is not
//!   funded for the bar before its fill.

use std::collections::BTreeMap;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const EPOCH: i64 = 1_704_067_200; // 2024-01-01T00:00:00Z

fn iso(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Hour-`h` ISO timestamp.
fn at(h: i64) -> String {
    iso(EPOCH + h * 3600)
}

fn strict() -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None, fills: None, gas: None, signals: None, ordering: None,
        data: None, perps: None, yield_: None,
    }
}

/// strict_v1 but tolerant of an interior candle gap (warn, not fail) so a test can
/// exercise a real data gap in the tick clock.
fn strict_warn_missing() -> SimulationPolicy {
    use catalyst_contracts::policy::DataPolicy;
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None, fills: None, gas: None, signals: None, ordering: None,
        data: Some(DataPolicy {
            missing_required: Some("warn".to_string()),
            missing_optional: None,
        }),
        perps: None, yield_: None,
    }
}

/// strict_v1 + the default `level` signal trigger so threshold signals fire on
/// every bar the condition holds.
fn strict_with_level_signals() -> SimulationPolicy {
    use catalyst_contracts::policy::SignalPolicy;
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None, fills: None, gas: None,
        signals: Some(SignalPolicy {
            trigger: Some("level".to_string()),
            repeat: None,
            cooldown: None,
            max_count: None,
        }),
        ordering: None, data: None, perps: None, yield_: None,
    }
}

fn config(venue: &str, usdc: &str, end_h: i64) -> BacktestConfig {
    let mut bal = BTreeMap::new();
    bal.insert("USDC".to_string(), usdc.to_string());
    let mut init = BTreeMap::new();
    init.insert(venue.to_string(), bal);
    BacktestConfig {
        start: at(0),
        end: at(end_h),
        interval: "1h".to_string(),
        initial_portfolio: init,
        execution: None,
    }
}

fn graph(v: Value) -> Graph {
    serde_json::from_value(v).unwrap()
}

fn count(t: &SimulationTrace, kind: &str) -> usize {
    t.events.iter().filter(|e| e.event_type == kind).count()
}

fn first<'a>(t: &'a SimulationTrace, kind: &str) -> &'a catalyst_contracts::trace::Event {
    t.events.iter().find(|e| e.event_type == kind).expect("event present")
}

fn detail_str(e: &catalyst_contracts::trace::Event, key: &str) -> String {
    e.detail.as_ref().unwrap().get(key).unwrap().as_str().unwrap().to_string()
}

/// Parse a decimal-string balance to f64; a missing key is zero.
fn bal(balances: &BTreeMap<String, String>, asset: &str) -> f64 {
    balances.get(asset).map(|s| s.parse::<f64>().unwrap()).unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// (1) DATA GAP between the decision bar and the next available bar.
// ---------------------------------------------------------------------------

/// An ETH bundle whose candles sit at the given hour offsets (a gap = a skipped
/// hour). Plus a perp funding series at every hour so we can verify funding spans
/// the gap. Both series share the `hyperliquid` venue.
fn gap_bundle(hours: &[i64], opens_closes: &[(&str, &str)]) -> MarketDataBundle {
    let candles: Vec<Value> = hours
        .iter()
        .zip(opens_closes)
        .map(|(h, (o, c))| json!({"ts": iso(EPOCH + h * 3600), "open": o, "high": o, "low": o, "close": c}))
        .collect();
    // Hourly funding at every hour from the first to the last candle hour.
    let last = *hours.last().unwrap();
    let funding: Vec<Value> = (1..=last)
        .map(|h| json!({"ts": iso(EPOCH + h * 3600), "rate": "0.001"}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": iso(EPOCH), "end": iso(EPOCH + (last + 1) * 3600),
        "candles": [{"venue": "hyperliquid", "symbol": "ETH", "quote": "USD", "points": candles}],
        "funding": [{"venue": "hyperliquid", "symbol": "ETH", "points": funding}],
        "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

#[test]
fn gap_market_fill_lands_on_next_actual_bar_open_and_funding_spans_gap() {
    // Decision bar at 0h. The 1h candle is MISSING (data gap); the next ACTUAL
    // candle is at 3h. A market perp decided at 0h must fill at the next actual
    // bar's OPEN (3h open = 2300), be booked at the 3h snapshot, and funding must
    // start only AFTER the fill (so no funding at the fill bar, and the first
    // funding charge — at the bar after — covers the elapsed window since fill,
    // not since the decision bar).
    let g = graph(json!({
        "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
            "config": {"symbol": "ETH", "side": "long", "size_usd": "1000", "leverage": "1",
                       "chain": "hyperliquid", "order_type": "market", "reduce_only": false}}],
        "edges": []
    }));
    // candles at 0h, 3h, 4h (1h and 2h are gapped away).
    let trace = run(&SimulationInput {
        graph: g,
        config: config("hyperliquid", "2000", 5),
        policy: strict_warn_missing(),
        market_data: gap_bundle(
            &[0, 3, 4],
            &[("2000", "2000"), ("2300", "2300"), ("2400", "2400")],
        ),
    })
    .unwrap();

    // The fill is booked on the next ACTUAL bar (3h), at its OPEN (2300 + 10bps).
    let exec = first(&trace, "action_executed");
    assert_eq!(
        exec.ts,
        at(3),
        "#116 gap: the market fill must be booked on the next ACTUAL bar (3h), not the \
         gapped/decision bar; got ts {}",
        exec.ts
    );
    assert_eq!(
        detail_str(exec, "price"),
        "2302.300",
        "#116 gap: fill price must be the next ACTUAL bar's open (2300) + 10bps slippage"
    );

    // The decision-bar (0h) snapshot has NO perp position; it appears at the 3h snapshot.
    assert!(
        trace.snapshots[0].portfolio.as_ref().unwrap().perp_positions.is_empty(),
        "#116 gap: the decision-bar (0h) snapshot must carry no perp position"
    );
    // snapshots are 0h, 3h, 4h (one per actual tick).
    let fill_snap = &trace.snapshots[1];
    assert_eq!(
        fill_snap.ts,
        at(3),
        "#116 gap: the second snapshot must be the next actual bar (3h)"
    );
    assert_eq!(
        fill_snap.portfolio.as_ref().unwrap().perp_positions.len(),
        1,
        "#116 gap: the perp position must appear on the fill bar (3h)"
    );

    // Funding: the first funding charge is the bar AFTER the fill (4h), and it
    // covers only the elapsed window since the fill (3h->4h = 1 hour = one 0.001
    // point), NOT the whole gap since the decision bar (which would be 2h->4h).
    let fe: Vec<_> = trace.events.iter().filter(|e| e.event_type == "funding_applied").collect();
    assert!(!fe.is_empty(), "#116 gap: expected funding after the fill");
    assert_eq!(
        fe[0].ts,
        at(4),
        "#116 gap: the position is funded only from the bar after its fill (4h), never \
         for the gap before the fill; got first funding at {}",
        fe[0].ts
    );
    assert_eq!(
        detail_str(fe[0], "rate"),
        "0.001",
        "#116 gap: the first funding charge covers only the 3h->4h elapsed hour (one 0.001 \
         point), not the gapped window before the fill"
    );
}

// ---------------------------------------------------------------------------
// (2) market -> market: the downstream hop defers ONE MORE bar.
// ---------------------------------------------------------------------------

fn simple_bundle(bars: &[(&str, &str)]) -> MarketDataBundle {
    let points: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, c))| json!({"ts": at(i as i64), "open": o, "high": o, "low": o, "close": c}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": at(0), "end": at(bars.len() as i64),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

#[test]
fn market_then_market_downstream_defers_one_more_bar() {
    // An initial market buy (USDC->ETH) at bar 0, with a downstream market sell
    // (ETH->USDC) wired after it. The upstream market defers to bar 1's open and
    // books there. The downstream market is DECIDED on bar 1 (its parent's fill
    // bar), so the same next_open rule defers it again: it fills at bar 2's open.
    // One bar per hop — a chained market never fills on the bar it was decided.
    let g = graph(json!({
        "nodes": [
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "200", "chain": "base"}},
            {"id": "sell", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "ETH", "to_asset": "USDC", "amount": "0.05", "chain": "base"}}
        ],
        "edges": [{"from": "buy", "to": "sell"}]
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 3),
        policy: strict(),
        market_data: simple_bundle(&[("2000", "2000"), ("2100", "2100"), ("2200", "2200")]),
    })
    .unwrap();

    // Both market swaps execute: the buy books on bar 1, the sell on bar 2.
    let execs: Vec<_> = trace.events.iter().filter(|e| e.event_type == "action_executed").collect();
    assert_eq!(
        execs.len(),
        2,
        "#116 chain: both the upstream and downstream market swaps must execute"
    );
    let buy_exec = execs.iter().find(|e| e.node_id.as_deref() == Some("buy")).unwrap();
    assert_eq!(
        buy_exec.ts,
        at(1),
        "#116 chain: the upstream market buy decided on bar 0 must book on bar 1; got {}",
        buy_exec.ts
    );
    let sell_exec = execs.iter().find(|e| e.node_id.as_deref() == Some("sell")).unwrap();
    assert_eq!(
        sell_exec.ts,
        at(2),
        "#116 chain: the downstream market sell is decided on bar 1 (its parent's fill \
         bar) and must defer one more bar to book on bar 2 — one bar per hop; got {}",
        sell_exec.ts
    );

    // Each hop announces its deferral on its own decision bar.
    let defers: Vec<_> = trace.events.iter().filter(|e| e.event_type == "order_deferred").collect();
    assert_eq!(defers.len(), 2, "#116 chain: each market hop emits one order_deferred");
    assert_eq!(defers[0].ts, at(0), "#116 chain: the buy defers on its decision bar (0h)");
    assert_eq!(defers[0].node_id.as_deref(), Some("buy"));
    assert_eq!(defers[1].ts, at(1), "#116 chain: the sell defers on ITS decision bar (1h)");
    assert_eq!(defers[1].node_id.as_deref(), Some("sell"));

    // Decision bar (0h) ledger untouched.
    let b0 = &trace.snapshots[0].portfolio.as_ref().unwrap().balances["base"];
    assert_eq!(
        b0["USDC"].to_string(),
        "1000",
        "#116 chain: the decision bar (0h) must be untouched — nothing fills there"
    );
    assert_eq!(bal(b0, "ETH"), 0.0, "#116 chain: no ETH on the decision bar");

    // Bar 1: only the buy has filled (ETH acquired, sell not yet executed).
    let b1 = &trace.snapshots[1].portfolio.as_ref().unwrap().balances["base"];
    let eth_after_buy = bal(b1, "ETH");
    assert!(eth_after_buy > 0.0, "#116 chain: the buy's ETH lands on bar 1");

    // Bar 2: the sell has filled — ETH reduced by exactly the 0.05 sold.
    let b2 = &trace.snapshots[2].portfolio.as_ref().unwrap().balances["base"];
    assert!(
        (bal(b2, "ETH") - (eth_after_buy - 0.05)).abs() < 1e-9,
        "#116 chain: the sell's 0.05 ETH leaves on bar 2; bar1 ETH {} bar2 ETH {}",
        eth_after_buy,
        bal(b2, "ETH")
    );
}

// ---------------------------------------------------------------------------
// (3) yield deposit (decision bar) -> market (next bar).
// ---------------------------------------------------------------------------

fn yield_and_candle_bundle(bars: &[(&str, &str)]) -> MarketDataBundle {
    let points: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, c))| json!({"ts": at(i as i64), "open": o, "high": o, "low": o, "close": c}))
        .collect();
    let yields: Vec<Value> = (0..bars.len() as i64)
        .map(|h| json!({"ts": at(h), "apr": "0.05"}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": at(0), "end": at(bars.len() as i64),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [],
        "yields": [{"protocol": "aave", "asset": "USDC", "chain": "base", "pool": "usdc", "points": yields}],
        "providers": [], "warnings": []
    }))
    .unwrap()
}

#[test]
fn yield_deposit_executes_decision_bar_then_market_fills_next_bar() {
    // A yield deposit (decision-bar action) with a downstream market swap. The
    // deposit executes on the decision bar (0h) — unchanged — while the market
    // swap defers and books on bar 1.
    let g = graph(json!({
        "nodes": [
            {"id": "deposit", "kind": "action", "subtype": "yield_deposit",
             "config": {"chain": "base", "protocol": "aave", "asset": "USDC",
                        "pool": "usdc", "amount": "100"}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "200", "chain": "base"}}
        ],
        "edges": [{"from": "deposit", "to": "buy"}]
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 3),
        policy: strict(),
        market_data: yield_and_candle_bundle(&[("2000", "2000"), ("2100", "2100"), ("2200", "2200")]),
    })
    .unwrap();

    // The yield deposit executes; find its action_executed event and confirm it is
    // on the decision bar (0h).
    let deposit_exec = trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed" && e.node_id.as_deref() == Some("deposit"))
        .expect("#116 yield-then-market: the deposit must execute");
    assert_eq!(
        deposit_exec.ts,
        at(0),
        "#116 yield-then-market: the yield deposit must execute on the DECISION bar (0h), \
         unchanged by the market-only deferral; got {}",
        deposit_exec.ts
    );

    // The downstream market swap defers and books on bar 1.
    let buy_exec = trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed" && e.node_id.as_deref() == Some("buy"))
        .expect("#116 yield-then-market: the market swap must execute");
    assert_eq!(
        buy_exec.ts,
        at(1),
        "#116 yield-then-market: the market swap must defer to and book on bar 1; got {}",
        buy_exec.ts
    );

    // Decision-bar snapshot reflects the deposit (cash reduced by 100, no ETH yet).
    let b0 = &trace.snapshots[0].portfolio.as_ref().unwrap().balances["base"];
    assert!(
        bal(b0, "USDC") <= 900.0,
        "#116 yield-then-market: the 100 deposit must leave the cash balance on the decision \
         bar; USDC = {}",
        b0["USDC"]
    );
    assert_eq!(
        bal(b0, "ETH"),
        0.0,
        "#116 yield-then-market: no ETH on the decision bar — the swap fills on bar 1"
    );
    // The position appears on bar 1.
    let b1 = &trace.snapshots[1].portfolio.as_ref().unwrap().balances["base"];
    assert!(
        bal(b1, "ETH") > 0.0,
        "#116 yield-then-market: the ETH position must appear on the fill bar (1h)"
    );
}

// ---------------------------------------------------------------------------
// (4) two signals on the same bar -> two market orders, both defer.
// ---------------------------------------------------------------------------

#[test]
fn two_signals_same_bar_both_market_orders_fill_next_bar() {
    // Two independent threshold signals, both true on bar 0, each driving its own
    // market swap. Both defer and fill on bar 1. (`level` trigger fires whenever
    // the condition holds; both conditions hold only on bar 0 here, then go false.)
    let g = graph(json!({
        "nodes": [
            {"id": "below2500", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "price", "symbol": "ETH"},
                        "operator": "<", "reference": {"const": "2500"}}},
            {"id": "below1800", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "price", "symbol": "ETH"},
                        "operator": "<", "reference": {"const": "1800"}}},
            {"id": "buyA", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base"}},
            {"id": "buyB", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "150", "chain": "base"}}
        ],
        "edges": [
            {"from": "below2500", "to": "buyA"},
            {"from": "below1800", "to": "buyB"}
        ]
    }));
    // bar 0 at 1700 satisfies BOTH thresholds (< 2500 and < 1800); later bars at
    // 3000 satisfy NEITHER, so each signal fires exactly once (on bar 0) and the
    // later bars still provide a next bar for the bar-0 fills.
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 3),
        policy: strict_with_level_signals(),
        market_data: simple_bundle(&[("1700", "1700"), ("3000", "3000"), ("3000", "3000")]),
    })
    .unwrap();

    // Both signals fired on bar 0.
    assert_eq!(
        count(&trace, "signal_fired"),
        2,
        "#116 two-signals: both threshold signals must fire on bar 0"
    );
    // Both market swaps execute, both booked on bar 1.
    let execs: Vec<_> = trace.events.iter().filter(|e| e.event_type == "action_executed").collect();
    assert_eq!(
        execs.len(),
        2,
        "#116 two-signals: both market swaps must execute (one per signal)"
    );
    for e in &execs {
        assert_eq!(
            e.ts,
            at(1),
            "#116 two-signals: each market swap decided on bar 0 must fill on bar 1; got {} \
             for node {:?}",
            e.ts, e.node_id
        );
    }
    // Decision bar (0h) untouched: no ETH, full cash.
    let b0 = &trace.snapshots[0].portfolio.as_ref().unwrap().balances["base"];
    assert_eq!(b0["USDC"].to_string(), "1000", "#116 two-signals: decision bar cash untouched");
    assert_eq!(bal(b0, "ETH"), 0.0, "#116 two-signals: no ETH on the decision bar");
    // Both positions land on bar 1.
    let b1 = &trace.snapshots[1].portfolio.as_ref().unwrap().balances["base"];
    assert!(bal(b1, "ETH") > 0.0, "#116 two-signals: ETH from both fills lands on bar 1");
    assert!(bal(b1, "USDC") < 1000.0, "#116 two-signals: cash debited on the fill bar");
}

// ---------------------------------------------------------------------------
// (5) sibling LIMIT rests from decision bar while sibling MARKET defers.
// ---------------------------------------------------------------------------

fn ohlc_bundle(bars: &[(&str, &str, &str, &str)]) -> MarketDataBundle {
    let points: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| json!({"ts": at(i as i64), "open": o, "high": h, "low": l, "close": c}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": at(0), "end": at(bars.len() as i64),
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

#[test]
fn sibling_limit_rests_from_decision_bar_while_market_defers() {
    // One signal-less graph with two initial actions: a LIMIT buy (placed on the
    // decision bar, rests immediately, eligible from bar 1) and a sibling MARKET
    // buy (deferred to bar 1's open). The limit placement is UNCHANGED by #116; it
    // is placed on bar 0 (an order_placed event at 0h) while the market action
    // does NOT execute on bar 0.
    let g = graph(json!({
        "nodes": [
            {"id": "limitbuy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base",
                        "order_type": "limit", "limit_price": "1900"}},
            {"id": "marketbuy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base"}}
        ],
        "edges": []
    }));
    // bar 0 trades ~2000; bar 1 dips to a low of 1850, touching the 1900 limit.
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 3),
        policy: strict(),
        market_data: ohlc_bundle(&[
            ("2000", "2010", "1990", "2000"),
            ("1980", "1985", "1850", "1900"),
            ("1900", "1910", "1890", "1900"),
        ]),
    })
    .unwrap();

    // The limit order is PLACED on the decision bar (0h) — unchanged behavior.
    let placed = first(&trace, "order_placed");
    assert_eq!(
        placed.ts,
        at(0),
        "#116 sibling-limit: the limit order must be PLACED on the decision bar (0h), \
         unchanged by the market-only deferral; got {}",
        placed.ts
    );
    assert_eq!(placed.node_id.as_deref(), Some("limitbuy"));

    // The sibling MARKET action does NOT execute on the decision bar; it books on bar 1.
    let market_exec = trace
        .events
        .iter()
        .find(|e| e.event_type == "action_executed" && e.node_id.as_deref() == Some("marketbuy"))
        .expect("#116 sibling-limit: the market swap must eventually execute");
    assert_eq!(
        market_exec.ts,
        at(1),
        "#116 sibling-limit: the sibling MARKET swap must defer to bar 1; got {}",
        market_exec.ts
    );
    assert_eq!(
        trace
            .events
            .iter()
            .filter(|e| e.event_type == "action_executed" && e.ts == at(0))
            .count(),
        0,
        "#116 sibling-limit: nothing should execute as a MARKET fill on the decision bar"
    );

    // The limit fills on bar 1 (touched at 1900), confirming it rested from bar 0.
    let filled = first(&trace, "order_filled");
    assert_eq!(
        filled.ts,
        at(1),
        "#116 sibling-limit: the resting limit must fill on bar 1 (the first eligible bar \
         after placement), confirming it rested from the decision bar; got {}",
        filled.ts
    );
}

// ---------------------------------------------------------------------------
// (6) the deferred position is NOT funded for the bar before its fill.
// ---------------------------------------------------------------------------

fn perp_funding_bundle(n: i64) -> MarketDataBundle {
    let candles: Vec<Value> = (0..n)
        .map(|h| json!({"ts": at(h), "open": "2000", "high": "2000", "low": "2000", "close": "2000"}))
        .collect();
    // Hourly funding at every hour 1..n-1 (a point on every bar after the first).
    let funding: Vec<Value> = (1..n)
        .map(|h| json!({"ts": at(h), "rate": "0.001"}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": at(0), "end": at(n),
        "candles": [{"venue": "hyperliquid", "symbol": "ETH", "quote": "USD", "points": candles}],
        "funding": [{"venue": "hyperliquid", "symbol": "ETH", "points": funding}],
        "gas": [], "yields": [], "providers": [], "warnings": []
    }))
    .unwrap()
}

#[test]
fn deferred_perp_not_funded_for_bar_before_its_fill() {
    // A market perp decided on bar 0 books on bar 1. There is a funding point on
    // every bar after the first (bars 1, 2, 3). Because the position does not
    // exist until bar 1, it must NOT be funded for bar 1's accrual window (which
    // belongs to the pre-fill interval 0h->1h). The FIRST funding charge must be
    // bar 2 (the first full bar the position is actually held), never bar 1.
    let trace = run(&SimulationInput {
        graph: graph(json!({
            "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
                "config": {"symbol": "ETH", "side": "long", "size_usd": "1000", "leverage": "1",
                           "chain": "hyperliquid", "order_type": "market", "reduce_only": false}}],
            "edges": []
        })),
        config: config("hyperliquid", "2000", 4),
        policy: strict(),
        market_data: perp_funding_bundle(4),
    })
    .unwrap();

    // The perp books on bar 1.
    let exec = first(&trace, "action_executed");
    assert_eq!(exec.ts, at(1), "#116 no-pre-fill-funding: the perp must book on bar 1");

    // No funding is charged on or before the fill bar (bar 1): the position did
    // not exist for the 0h->1h interval.
    let fe: Vec<_> = trace.events.iter().filter(|e| e.event_type == "funding_applied").collect();
    assert!(!fe.is_empty(), "#116 no-pre-fill-funding: expected funding after the fill");
    assert!(
        fe.iter().all(|e| e.ts != at(1) && e.ts != at(0)),
        "#116 no-pre-fill-funding: the deferred position must NOT be funded for the bar \
         before its fill (0h) or the fill bar's pre-fill window (1h); funding ts = {:?}",
        fe.iter().map(|e| e.ts.clone()).collect::<Vec<_>>()
    );
    assert_eq!(
        fe[0].ts,
        at(2),
        "#116 no-pre-fill-funding: the first funding charge must be bar 2 (the first full \
         bar the position is held), not the fill bar; got {}",
        fe[0].ts
    );
    // Sanity: the position never existed on the decision-bar snapshot.
    assert!(
        trace.snapshots[0].portfolio.as_ref().unwrap().perp_positions.is_empty(),
        "#116 no-pre-fill-funding: no perp on the decision-bar (0h) snapshot"
    );
}
