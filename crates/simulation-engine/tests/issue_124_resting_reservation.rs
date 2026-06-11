//! ISSUE #124 — resting orders reserve the balance their fill will spend.
//!
//! A reservation is a side-table earmark, NOT a debit: placement never moves a
//! balance, so equity is unchanged by construction while the order rests
//! (owned-but-committed cash is still equity). What changes is the *spendable*
//! figure: the strict debit guard and sizing read `available = balance −
//! reserved`, so a later action can't raid cash a resting order has committed,
//! and an unaffordable placement is rejected up front instead of resting
//! doomed. Reservations release on fill, TIF expiry, fill-time rejection, and
//! run end; deferred `next_open` market orders (#116) use the same mechanism
//! with their amounts (including the `"all"` sentinel) frozen at the decision
//! bar.

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

/// A bundle with an ETH candle path of explicit (open, high, low, close) bars
/// and an optional per-bar gas series (USD).
fn bundle(venue: &str, bars: &[(&str, &str, &str, &str)], gas: &[(i64, &str)]) -> MarketDataBundle {
    let points: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| json!({"ts": ts(i as i64), "open": o, "high": h, "low": l, "close": c}))
        .collect();
    let gas_points: Vec<Value> =
        gas.iter().map(|(i, usd)| json!({"ts": ts(*i), "gas_usd": usd})).collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h",
        "start": ts(0),
        "end": ts(bars.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [],
        "gas": [{"chain": venue, "points": gas_points}],
        "yields": [],
        "providers": [],
        "warnings": []
    }))
    .unwrap()
}

fn config(venue: &str, usdc: &str, n_ticks: i64) -> BacktestConfig {
    let mut venue_balances = BTreeMap::new();
    venue_balances.insert("USDC".to_string(), usdc.to_string());
    let mut initial = BTreeMap::new();
    initial.insert(venue.to_string(), venue_balances);
    BacktestConfig {
        start: START.to_string(),
        end: ts(n_ticks),
        interval: "1h".to_string(),
        initial_portfolio: initial,
        execution: None,
    }
}

fn policy(v: Value) -> SimulationPolicy {
    serde_json::from_value(v).unwrap()
}

/// strict_v1, same-bar close fills, no slippage/fees/gas: deterministic
/// balance math for reservation arithmetic.
fn close_no_costs() -> SimulationPolicy {
    policy(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "fills": {"price_selection": "close",
                  "slippage": {"model": "none"}, "fees": {"model": "none"}},
        "gas": {"model": "none"}
    }))
}

/// strict_v1 (next_open deferral, #116) with no slippage/fees/gas.
fn next_open_no_costs() -> SimulationPolicy {
    policy(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "fills": {"slippage": {"model": "none"}, "fees": {"model": "none"}},
        "gas": {"model": "none"}
    }))
}

/// Plain strict_v1 (fee 5 bps; hyperliquid carries no gas).
fn strict() -> SimulationPolicy {
    policy(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1"
    }))
}

fn graph(value: Value) -> Graph {
    serde_json::from_value(value).unwrap()
}

fn count(trace: &SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

fn first(trace: &SimulationTrace, kind: &str) -> catalyst_contracts::trace::Event {
    trace.events.iter().find(|e| e.event_type == kind).cloned().expect("event present")
}

fn detail<'a>(e: &'a catalyst_contracts::trace::Event, key: &str) -> &'a Value {
    e.detail.as_ref().unwrap().get(key).unwrap_or(&Value::Null)
}

/// A USDC->ETH limit buy on `base` for `amount`, limit 1900 (market ~2000).
fn limit_buy(id: &str, amount: &str) -> Value {
    json!({"id": id, "kind": "action", "subtype": "swap",
        "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": amount, "chain": "base",
                   "order_type": "limit", "limit_price": "1900"}})
}

fn market_buy(id: &str, amount: Value) -> Value {
    json!({"id": id, "kind": "action", "subtype": "swap",
        "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": amount, "chain": "base",
                   "order_type": "market"}})
}

/// A price signal that fires when ETH's close exceeds `threshold`.
fn eth_above(id: &str, threshold: &str) -> Value {
    json!({"id": id, "kind": "signal", "subtype": "price_threshold",
        "config": {"symbol": "ETH", "operator": ">", "threshold": threshold}})
}

const FLAT: (&str, &str, &str, &str) = ("2000", "2010", "1990", "2000");
const DIP: (&str, &str, &str, &str) = ("1980", "1985", "1850", "1900");

// --- 1. equity is identical across the placement bar (earmark, not debit) ---

#[test]
fn placement_does_not_move_equity_or_balances() {
    // A resting buy commits 600 of 1000 USDC, but placement only earmarks: the
    // cash is still owned, still in the portfolio, still equity. (This pins the
    // corrected #124 framing: equity was never over-counted by placement —
    // owned-but-committed cash IS equity.)
    let g = graph(json!({ "nodes": [limit_buy("buy", "600")], "edges": [] }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: close_no_costs(),
        market_data: bundle("base", &[FLAT, FLAT], &[]),
    })
    .unwrap();
    assert_eq!(count(&trace, "order_placed"), 1);
    let placed = first(&trace, "order_placed");
    assert_eq!(detail(&placed, "reserved_asset"), "USDC");
    assert_eq!(detail(&placed, "reserved_amount"), "600");
    // Equity and the portfolio are untouched on the placement bar (and every bar
    // after — the order never fills here).
    for snap in &trace.snapshots {
        assert_eq!(snap.equity_usd, "1000", "reservation must not move equity at {}", snap.ts);
        assert_eq!(snap.portfolio.as_ref().unwrap().balances["base"]["USDC"], "1000");
    }
}

// --- 2. reserved cash cannot be spent by a later market order ---

#[test]
fn market_order_cannot_spend_reserved_cash() {
    // The resting buy earmarks 600 of 1000; a market buy for 500 needs more
    // than the 400 available and is rejected, naming the reservation.
    let g = graph(json!({
        "nodes": [limit_buy("limit", "600"), eth_above("sig", "1"), market_buy("spend", json!("500"))],
        "edges": [{"from": "sig", "to": "spend"}]
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: close_no_costs(),
        market_data: bundle("base", &[FLAT, FLAT], &[]),
    })
    .unwrap();
    assert_eq!(count(&trace, "order_placed"), 1);
    assert_eq!(count(&trace, "action_executed"), 0, "the 500 buy must not fill");
    assert_eq!(count(&trace, "action_rejected"), 1);
    let rejected = first(&trace, "action_rejected");
    let reason = rejected.reason.as_deref().unwrap();
    assert!(
        reason.contains("reserved by resting orders"),
        "rejection must name the reservation: {reason}"
    );
    assert!(reason.contains("600"), "rejection names the reserved figure: {reason}");
}

// --- 3. pct_balance sizes against available (balance − reserved) ---

#[test]
fn pct_balance_sizes_against_available_not_raw_balance() {
    // 600 of 1000 reserved -> available 400; a 50% pct_balance buy must size to
    // 200 (50% of available), not 500 (50% of the raw balance).
    let g = graph(json!({
        "nodes": [limit_buy("limit", "600"), eth_above("sig", "1"),
                  market_buy("spend", json!({"basis": "pct_balance", "value": "50"}))],
        "edges": [{"from": "sig", "to": "spend"}]
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: close_no_costs(),
        market_data: bundle("base", &[FLAT, FLAT], &[]),
    })
    .unwrap();
    assert_eq!(count(&trace, "action_rejected"), 0);
    assert_eq!(count(&trace, "action_executed"), 1);
    let exec = first(&trace, "action_executed");
    assert_eq!(detail(&exec, "value_usd"), "200", "50% of the 400 available");
    // 1000 − 200 spent (600 still only earmarked, not moved).
    let b0 = &trace.snapshots[0].portfolio.as_ref().unwrap().balances["base"];
    assert_eq!(b0["USDC"], "800");
    assert_eq!(b0["ETH"], "0.1"); // 200 / 2000
}

// --- 4. the reservation is released on fill (the fill spends the freed funds) ---

#[test]
fn fill_releases_the_reservation_for_its_own_spend_and_downstream() {
    // The 600 buy fills at 1900 on bar 1; its downstream market buy then spends
    // the remaining 400 — possible only if the earmark was released before the
    // fill and is gone afterwards.
    let g = graph(json!({
        "nodes": [limit_buy("limit", "600"), market_buy("spend", json!("400"))],
        "edges": [{"from": "limit", "to": "spend"}]
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: close_no_costs(),
        market_data: bundle("base", &[FLAT, DIP], &[]),
    })
    .unwrap();
    assert_eq!(count(&trace, "order_filled"), 1);
    assert_eq!(count(&trace, "action_rejected"), 0);
    assert_eq!(count(&trace, "action_executed"), 1, "the downstream 400 buy executes");
    let final_balances = &trace.final_portfolio.balances["base"];
    assert!(!final_balances.contains_key("USDC"), "all 1000 USDC spent (zeros dropped)");
    let eth: f64 = final_balances["ETH"].parse().unwrap();
    // 600/1900 (limit fill) + 400/1900 (downstream at bar 1's close 1900).
    assert!((eth - 1000.0 / 1900.0).abs() < 1e-9, "ETH was {eth}");
}

// --- 5. the reservation is released on TIF expiry ---

#[test]
fn expiry_releases_the_reservation() {
    // The buy never touches and expires after 1 bar; on the expiry bar the full
    // 1000 is spendable again.
    let mut node = limit_buy("limit", "600");
    node["config"]["time_in_force"] = json!("good_til_bars");
    node["config"]["expire_after_bars"] = json!(1);
    let g = graph(json!({
        "nodes": [node, eth_above("sig", "2050"), market_buy("spend", json!("1000"))],
        "edges": [{"from": "sig", "to": "spend"}]
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 3),
        policy: close_no_costs(),
        // The signal fires on bar 2 (close 2100), the same bar the order expires.
        market_data: bundle("base", &[FLAT, FLAT, ("2100", "2110", "2090", "2100")], &[]),
    })
    .unwrap();
    assert_eq!(count(&trace, "order_expired"), 1);
    assert_eq!(first(&trace, "order_expired").ts, ts(2));
    assert_eq!(count(&trace, "action_rejected"), 0, "the freed 1000 is spendable on bar 2");
    assert_eq!(count(&trace, "action_executed"), 1);
}

// --- 6. a starved fill (gas drift) rejects loudly and releases ---

#[test]
fn gas_drift_starves_the_fill_with_run_warning_and_releases() {
    // Historical gas at placement (bar 0) is 0.25 -> the 999 buy reserves
    // 999.25 of 1000 (fees off). By the fill bar gas has spiked to 500, so the
    // fill needs 1499 — the one shortfall vector reservations can't close.
    // The order must reject loudly (order_rejected + run warning) and release,
    // leaving the full 1000 spendable (the bar-2 buy at sane gas proves it).
    let p = policy(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "fills": {"price_selection": "close",
                  "slippage": {"model": "none"}, "fees": {"model": "none"}}
        // gas stays strict_v1's historical_fee_history
    }));
    let g = graph(json!({
        "nodes": [limit_buy("limit", "999"), eth_above("sig", "2050"), market_buy("spend", json!("999"))],
        "edges": [{"from": "sig", "to": "spend"}]
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 3),
        policy: p,
        market_data: bundle(
            "base",
            &[FLAT, DIP, ("2100", "2110", "2090", "2100")],
            &[(0, "0.25"), (1, "500"), (2, "0.25")],
        ),
    })
    .unwrap();
    assert_eq!(count(&trace, "order_placed"), 1, "999.25 <= 1000 at placement gas");
    assert_eq!(count(&trace, "order_filled"), 0);
    assert_eq!(count(&trace, "order_rejected"), 1, "starved by the gas spike at fill");
    assert!(
        trace.warnings.iter().any(|w| w.contains("starved at fill")),
        "starvation must surface as a run-level warning: {:?}",
        trace.warnings
    );
    // Released: the bar-2 buy (999 + 0.25 gas) succeeds against the freed 1000.
    assert_eq!(count(&trace, "action_executed"), 1);
    assert_eq!(count(&trace, "action_rejected"), 0);
}

// --- 7. a second over-committing limit is rejected AT PLACEMENT ---

#[test]
fn second_overcommitting_limit_rejected_at_placement() {
    let g = graph(json!({
        "nodes": [limit_buy("limit1", "600"), limit_buy("limit2", "600")],
        "edges": []
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: close_no_costs(),
        market_data: bundle("base", &[FLAT, FLAT], &[]),
    })
    .unwrap();
    assert_eq!(count(&trace, "order_placed"), 1, "only the first 600 fits in 1000");
    assert_eq!(count(&trace, "action_rejected"), 1, "the second rejects at placement, not at fill");
    let reason = first(&trace, "action_rejected").reason.unwrap();
    assert!(
        reason.contains("reserved by resting orders"),
        "placement rejection names the reservation: {reason}"
    );
}

// --- 8. allow_negative: reservations are inert ---

#[test]
fn allow_negative_keeps_reservations_inert() {
    // Two over-committing limits both place, and a market buy larger than the
    // whole balance still executes (driving USDC negative): reserve never
    // fails, the debit is unguarded — the explicit-debt policy is unchanged.
    let p = policy(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1",
        "balance": {"insufficient_balance": "allow_negative"},
        "fills": {"price_selection": "close",
                  "slippage": {"model": "none"}, "fees": {"model": "none"}},
        "gas": {"model": "none"}
    }));
    let g = graph(json!({
        "nodes": [limit_buy("limit1", "600"), limit_buy("limit2", "600"),
                  eth_above("sig", "1"), market_buy("spend", json!("1200"))],
        "edges": [{"from": "sig", "to": "spend"}]
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: p,
        market_data: bundle("base", &[FLAT, FLAT], &[]),
    })
    .unwrap();
    assert_eq!(count(&trace, "order_placed"), 2, "unaffordable limits still place");
    assert_eq!(count(&trace, "action_rejected"), 0);
    assert_eq!(count(&trace, "action_executed"), 1, "the 1200 buy overdraws unguarded");
    let usdc: f64 = trace.final_portfolio.balances["base"]["USDC"].parse().unwrap();
    assert_eq!(usdc, -200.0, "1000 − 1200; reservations never blocked the spend");
}

// --- 9. perp limits: open reserves margin+fee; reduce-only reserves nothing ---

#[test]
fn perp_limit_open_reserves_margin_plus_fee() {
    // strict_v1 fee 5 bps: 500 USD at 2x -> margin 250 + fee 0.25 = 250.25.
    let g = graph(json!({
        "nodes": [{"id": "open", "kind": "action", "subtype": "perp_order",
            "config": {"symbol": "ETH", "side": "long", "size_usd": "500", "leverage": "2",
                       "chain": "hyperliquid", "order_type": "limit", "limit_price": "1900",
                       "reduce_only": false}}],
        "edges": []
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 2),
        policy: strict(),
        market_data: bundle("hyperliquid", &[FLAT, FLAT], &[]),
    })
    .unwrap();
    let placed = first(&trace, "order_placed");
    assert_eq!(detail(&placed, "reserved_asset"), "USDC");
    assert_eq!(detail(&placed, "reserved_amount"), "250.25");
}

#[test]
fn perp_reduce_only_limit_reserves_nothing() {
    // Market long opens (deferred to bar 1 under strict next_open), then the
    // chained reduce-only take-profit places on bar 1 — credits only, so its
    // order_placed carries no reservation.
    let g = graph(json!({
        "nodes": [
            {"id": "open", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "long", "size_usd": "500", "leverage": "2",
                        "chain": "hyperliquid", "order_type": "market", "reduce_only": false}},
            {"id": "tp", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "short", "size_usd": "500",
                        "chain": "hyperliquid", "order_type": "limit", "limit_price": "2200",
                        "reduce_only": true}}
        ],
        "edges": [{"from": "open", "to": "tp"}]
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("hyperliquid", "1000", 2),
        policy: strict(),
        market_data: bundle("hyperliquid", &[FLAT, FLAT], &[]),
    })
    .unwrap();
    assert_eq!(count(&trace, "order_placed"), 1, "the take-profit placed");
    let placed = first(&trace, "order_placed");
    assert_eq!(placed.node_id.as_deref(), Some("tp"));
    assert_eq!(detail(&placed, "reserved_asset"), &Value::Null);
    assert_eq!(detail(&placed, "reserved_amount"), &Value::Null);
}

// --- 10. same-bar deferred next_open markets can't double-spend ---

#[test]
fn same_bar_deferred_markets_cannot_both_spend_the_balance() {
    // Two market buys of 600 each decided on the same bar under next_open
    // (#116 deferral): the first defers and reserves 600; the second would need
    // 600 of the 400 still available and is rejected AT THE DECISION BAR.
    let g = graph(json!({
        "nodes": [market_buy("buy1", json!("600")), market_buy("buy2", json!("600"))],
        "edges": []
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: next_open_no_costs(),
        market_data: bundle("base", &[FLAT, FLAT], &[]),
    })
    .unwrap();
    assert_eq!(count(&trace, "order_deferred"), 1);
    let deferred = first(&trace, "order_deferred");
    assert_eq!(deferred.ts, ts(0));
    assert_eq!(detail(&deferred, "reserved_asset"), "USDC");
    assert_eq!(detail(&deferred, "reserved_amount"), "600");
    let rejected = first(&trace, "action_rejected");
    assert_eq!(rejected.ts, ts(0), "rejected at the decision bar, not at fill");
    assert!(
        rejected.reason.as_deref().unwrap().contains("reserved by resting orders"),
        "rejection names the reservation: {:?}",
        rejected.reason
    );
    // Bar 1: the surviving order fills at the open and its reservation is gone.
    assert_eq!(count(&trace, "action_executed"), 1);
    let final_balances = &trace.final_portfolio.balances["base"];
    assert_eq!(final_balances["USDC"], "400");
    assert_eq!(final_balances["ETH"], "0.3"); // 600 / 2000 open
}

// --- the "all" sentinel freezes at the decision bar for queued orders ---

#[test]
fn deferred_all_freezes_at_the_decision_bar() {
    // An "all" market buy under next_open resolves to the decision bar's
    // available balance (1000) and reserves it; a same-bar sibling spending the
    // "same" full balance must therefore reject. Pre-#124 both deferred and
    // "all" re-read the balance at fill time.
    let g = graph(json!({
        "nodes": [market_buy("all_buy", json!("all")), market_buy("buy2", json!("100"))],
        "edges": []
    }));
    let trace = run(&SimulationInput {
        graph: g,
        config: config("base", "1000", 2),
        policy: next_open_no_costs(),
        market_data: bundle("base", &[FLAT, FLAT], &[]),
    })
    .unwrap();
    let deferred = first(&trace, "order_deferred");
    assert_eq!(detail(&deferred, "reserved_amount"), "1000", "\"all\" froze to 1000 at decision");
    assert_eq!(count(&trace, "action_rejected"), 1, "the sibling can't spend reserved cash");
    // Bar 1: the frozen 1000 fills at the open.
    assert_eq!(count(&trace, "action_executed"), 1);
    let final_balances = &trace.final_portfolio.balances["base"];
    assert!(!final_balances.contains_key("USDC"), "the full 1000 was spent");
    assert_eq!(final_balances["ETH"], "0.5"); // 1000 / 2000 open
}
