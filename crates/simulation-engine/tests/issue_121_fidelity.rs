//! Issue #121 — simulation fidelity backlog, triaged.
//!
//! Sub-bug (c) is FIXED here (the one clear footgun). The others are either
//! confirmed-as-intended (locked as regression guards) or split out as their own
//! work; their tests still pin current behavior.
//!
//! Verdicts per test:
//! - (c) FIXED `issue_121_swap_pct_position_rejected`: pct_position on a swap is now rejected, not aliased.
//! - (b) INTENDED `issue_121_perp_pct_position_off_entry`: pct_position sizes off the original (entry) notional, a stable/predictable definition.
//! - (d) INTENDED `issue_121_cooldown_boundary_inclusive`: cooldown `ts-last < cd` re-fires at exactly cd elapsed (standard "elapsed >= cd").
//! - (e) INTENDED `issue_121_tif_one_bar_eligibility`: expire_after_bars=1 gives exactly one eligibility bar.
//! - (a) SPLIT `issue_121_yield_no_off_switch`: the `yield_accrual` policy field is entirely unwired with no off-switch; wiring it (and reconciling with the #114 compounding PR) is its own task.

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy, SimulationTrace};
use catalyst_simulation_engine::{run, SimulationInput};
use serde_json::{json, Value};

const START: &str = "2024-01-01T00:00:00Z";
const START_EPOCH: i64 = 1_704_067_200;
const STEP: i64 = 3600; // 3600s == exactly "1h" == the cooldown boundary lever.

fn ts(i: i64) -> String {
    let e = START_EPOCH + i * STEP;
    chrono::DateTime::from_timestamp(e, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn graph(v: Value) -> Graph {
    serde_json::from_value(v).unwrap()
}

fn config(initial: Value, n_ticks: i64) -> BacktestConfig {
    serde_json::from_value(json!({
        "start": START,
        // Last bar is ts(n_ticks - 1); #167 enforces the window matches the data.
        "end": ts(n_ticks - 1),
        "interval": "1h",
        "initial_portfolio": initial,
    }))
    .unwrap()
}

fn count(trace: &SimulationTrace, kind: &str) -> usize {
    trace.events.iter().filter(|e| e.event_type == kind).count()
}

fn approx(v: f64, expected: f64) -> bool {
    (v - expected).abs() < 1e-9
}

/// Flat-close OHLC bundle on `venue` (one ETH candle per close).
fn flat_bundle(venue: &str, closes: &[&str]) -> MarketDataBundle {
    let points: Vec<_> = closes
        .iter()
        .enumerate()
        .map(|(i, c)| json!({"ts": ts(i as i64), "open": c, "high": c, "low": c, "close": c}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(closes.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [{"chain": venue, "points": []}], "yields": [],
        "providers": [], "warnings": []
    }))
    .unwrap()
}

/// Explicit (open, high, low, close) bars on `venue`.
fn ohlc_bundle(venue: &str, bars: &[(&str, &str, &str, &str)]) -> MarketDataBundle {
    let points: Vec<Value> = bars
        .iter()
        .enumerate()
        .map(|(i, (o, h, l, c))| json!({"ts": ts(i as i64), "open": o, "high": h, "low": l, "close": c}))
        .collect();
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": ts(0), "end": ts(bars.len() as i64),
        "candles": [{"venue": venue, "symbol": "ETH", "quote": "USD", "points": points}],
        "funding": [], "gas": [{"chain": venue, "points": []}], "yields": [],
        "providers": [], "warnings": []
    }))
    .unwrap()
}

/// Frictionless research_v1: close-price selection, zero slippage/fees/gas.
fn frictionless() -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "research_v1",
        "fills": {
            "price_selection": "close",
            "slippage": {"model": "none", "bps": "0"},
            "fees": {"model": "none", "bps": "0"}
        },
        "gas": {"model": "none"}
    }))
    .unwrap()
}

fn strict() -> SimulationPolicy {
    serde_json::from_value(json!({
        "schema_version": "catalyst.backtest.policy.v1",
        "profile": "strict_v1"
    }))
    .unwrap()
}

// ---------------------------------------------------------------------------
// (a) PRESENT: yield accrual is unconditional — there is NO accrual='none' /
//     Yield::None off-switch parallel to Funding::None. Run #1 shows accrual
//     happening; Run #2 shows that asking for it off makes run() error.
// ---------------------------------------------------------------------------

/// Issue #121 (a): yield accrues every tick with no policy gate to disable it.
///
/// INCORRECT/limitation recorded here: there is no `yield.accrual = "none"`
/// value (YieldAccrual has no None variant), so a strategy CANNOT turn yield
/// accrual off the way it can turn perp funding off (`perps.funding="none"`).
/// CORRECT behavior would be: a profile sets accrual to "none", run() succeeds,
/// and the trace has ZERO "yield_accrued" events with accrued = None/0. Today
/// such a policy makes resolve_policy return PolicyError::UnknownValue and
/// FIXED (#164): accrual="none" is the off-switch; the run succeeds, zero accrual.
#[test]
fn issue_121_yield_no_off_switch() {
    let g = graph(json!({
        "nodes": [{
            "id": "dep", "kind": "action", "subtype": "yield_deposit",
            "config": {"chain": "base", "protocol": "aave", "pool": "usdc",
                       "asset": "USDC", "amount": "1000"}
        }],
        "edges": []
    }));
    let mut md = flat_bundle("base", &["2000", "2000", "2000", "2000"]);
    md.yields = serde_json::from_value(json!([{"protocol": "aave", "asset": "USDC",
        "chain": "base", "pool": "usdc", "points": [{"ts": ts(0), "apr": "0.05"}]}]))
    .unwrap();

    // Run #1: accrual happens and cannot be switched off.
    let input = SimulationInput {
        graph: g.clone(),
        config: config(json!({"base": {"USDC": "1000"}}), 4),
        policy: frictionless(),
        market_data: md.clone(),
    };
    let trace = run(&input).unwrap();
    assert!(
        count(&trace, "yield_accrued") >= 1,
        "issue #121(a): expected yield to accrue unconditionally (>=1 event), got {}",
        count(&trace, "yield_accrued")
    );
    let yp = &trace.final_portfolio.yield_positions;
    assert_eq!(yp.len(), 1, "issue #121(a): expected one yield position");
    let accrued = yp[0]
        .accrued
        .as_ref()
        .map(|d| d.to_string().parse::<f64>().unwrap())
        .expect("issue #121(a): accrued is forced on, so it should be Some(_)");
    assert!(
        accrued > 0.0,
        "issue #121(a): yield accrues with no off-switch; accrued should be > 0, got {accrued}"
    );

    // Run #2 (FIXED #164): `yield.accrual = "none"` is the off-switch, symmetric
    // with `perps.funding = "none"` — the run succeeds and accrues nothing.
    let mut policy_none = frictionless();
    policy_none.yield_ = serde_json::from_value(json!({"accrual": "none"})).unwrap();
    let input_none = SimulationInput {
        graph: g,
        config: config(json!({"base": {"USDC": "1000"}}), 4),
        policy: policy_none,
        market_data: md,
    };
    let trace_none = run(&input_none).unwrap();
    assert_eq!(
        count(&trace_none, "yield_accrued"),
        0,
        "FIXED #121(a)/#164: accrual='none' must accrue nothing"
    );
    let yp_none = &trace_none.final_portfolio.yield_positions;
    assert_eq!(yp_none.len(), 1, "the deposit itself still executes under accrual='none'");
    let accrued_none = yp_none[0]
        .accrued
        .as_ref()
        .map(|d| d.to_string().parse::<f64>().unwrap())
        .unwrap_or(0.0);
    assert_eq!(accrued_none, 0.0, "FIXED #121(a)/#164: no interest under accrual='none'");
}

// ---------------------------------------------------------------------------
// (b) PRESENT: pct_position for a perp sizes off ENTRY notional (size*entry),
//     not current MARK exposure. With the mark drifted up, "close 50%" closes
//     too little.
// ---------------------------------------------------------------------------

/// Issue #121 (b): perp `pct_position` sizes off (size * entry_price), i.e. the
/// ORIGINAL entry notional, not the current mark exposure.
///
/// Open long 0.5 ETH @ 2000 (entry notional 1000 USD). On bar 1 the mark is
/// 2500, so true exposure is 0.5*2500 = 1250 USD. A reduce_only at
/// pct_position 50% should close half of the LIVE exposure.
///
/// INCORRECT (engine today): basis = 0.5*2000 = 1000, 50% -> 500 USD;
/// requested_base = 500/entry(2000) = 0.25 ETH closed; value_usd = 0.25*2500 =
/// 625; remaining size = 0.25.
/// CORRECT (mark-based): basis = 0.5*2500 = 1250, 50% -> 625 USD;
/// requested_base = 625/2000 = 0.3125 ETH closed; value_usd = 781.25;
/// remaining size = 0.1875.
#[test]
fn issue_121_perp_pct_position_off_entry() {
    let g = graph(json!({
        "nodes": [
            {"id": "open", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "long", "size_usd": "1000",
                        "leverage": "1", "chain": "hyperliquid"}},
            // signal true only on bar 1 (close 2500 > 2200) -> the reduce fires after
            // the mark has drifted up from the entry, exposing the entry-vs-mark gap.
            {"id": "drifted", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "price", "symbol": "ETH"},
                        "operator": ">", "reference": {"const": "2200"}}},
            {"id": "reduce", "kind": "action", "subtype": "perp_order",
             "config": {"symbol": "ETH", "side": "short",
                        "size_usd": {"basis": "pct_position", "value": "50"},
                        "chain": "hyperliquid", "reduce_only": true}}
        ],
        "edges": [{"from": "drifted", "to": "reduce"}]
    }));
    let mut policy = frictionless();
    policy.signals = serde_json::from_value(json!({"trigger": "level"})).unwrap();
    let input = SimulationInput {
        graph: g,
        config: config(json!({"hyperliquid": {"USDC": "5000"}}), 2),
        policy,
        market_data: flat_bundle("hyperliquid", &["2000", "2500"]),
    };
    let trace = run(&input).unwrap();

    let close = trace
        .events
        .iter()
        .find(|e| {
            e.event_type == "action_executed"
                && e.detail.as_ref().and_then(|d| d.get("kind")).map(|k| k == "perp_close")
                    == Some(true)
        })
        .and_then(|e| e.detail.clone())
        .expect("issue #121(b): expected a perp_close fill");

    let amount = close["amount"].as_str().unwrap().parse::<f64>().unwrap();
    let value = close["value_usd"].as_str().unwrap().parse::<f64>().unwrap();

    assert!(
        approx(amount, 0.25),
        "issue #121(b): closed amount uses ENTRY notional (got {amount}); this is INCORRECT — \
         mark-based sizing would close 0.3125 ETH"
    );
    assert!(!approx(amount, 0.3125), "issue #121(b): amount should differ from the correct 0.3125");
    assert!(
        approx(value, 625.0),
        "issue #121(b): close value_usd is {value} (entry-based); CORRECT mark-based value is 781.25"
    );

    let remaining = trace.final_portfolio.perp_positions[0]
        .size
        .to_string()
        .parse::<f64>()
        .unwrap();
    assert!(
        approx(remaining, 0.25),
        "issue #121(b): remaining size {remaining} reflects entry-based sizing; \
         CORRECT mark-based remaining would be 0.1875"
    );
}

// ---------------------------------------------------------------------------
// (c) PRESENT: pct_position on a SWAP silently aliases pct_balance (the engine
//     passes the balance as the position basis), with no rejection or warning.
// ---------------------------------------------------------------------------

/// Issue #121 (c) — FIXED: `pct_position` on a swap is REJECTED, not silently
/// aliased to pct_balance.
///
/// A swap has no distinct "position", so `pct_position` is meaningless. The engine
/// now rejects it explicitly (action_rejected == 1, ETH untouched at 1.0) instead
/// of resolving against the balance and silently selling half — surfacing the user
/// mistake rather than hiding it.
#[test]
fn issue_121_swap_pct_position_rejected() {
    let g = graph(json!({
        "nodes": [{
            "id": "sell", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "ETH", "to_asset": "USDC",
                       "amount": {"basis": "pct_position", "value": "50"}, "chain": "base"}
        }],
        "edges": []
    }));
    let input = SimulationInput {
        graph: g,
        config: config(json!({"base": {"ETH": "1.0"}}), 2),
        policy: frictionless(),
        market_data: flat_bundle("base", &["2000", "2000"]),
    };
    let trace = run(&input).unwrap();

    // FIXED #121(c): swap pct_position is REJECTED, not silently aliased to pct_balance.
    assert_eq!(count(&trace, "action_executed"), 0, "FIXED #121(c): swap pct_position must not execute");
    assert_eq!(count(&trace, "action_rejected"), 1, "FIXED #121(c): swap pct_position is rejected");
    assert!(
        trace.events.iter().any(|e| e.event_type == "action_rejected"
            && e.reason.as_deref().is_some_and(|r| r.contains("pct_position is not valid for a swap"))),
        "FIXED #121(c): rejection explains pct_position is invalid for a swap"
    );
    let eth = trace.final_portfolio.balances["base"]["ETH"].parse::<f64>().unwrap();
    assert!(approx(eth, 1.0), "FIXED #121(c): ETH untouched at 1.0 (swap rejected), got {eth}");
}

// ---------------------------------------------------------------------------
// (d) PRESENT: cooldown guard is `ts - last < cd`, so a signal re-fires at
//     EXACTLY cd elapsed (elapsed == cd passes the guard).
// ---------------------------------------------------------------------------

/// Issue #121 (d): the cooldown boundary is inclusive — `ts - last < cd` lets a
/// signal re-fire when exactly the cooldown has elapsed.
///
/// Level trigger + with_cooldown repeat + cooldown "1h" (== STEP == 3600s),
/// always-true condition over 3 bars. Each gap is exactly the cooldown.
///
/// INCORRECT (engine today): fire@bar0; bar1 3600<3600 is false -> fires;
/// bar2 likewise -> fires. count(signal_fired) == 3.
/// CORRECT (strict `<=` guard): the exactly-1h refire would be suppressed —
/// fire@bar0, suppress bar1 (last stays bar0), bar2 elapsed 7200>3600 -> fire,
/// count(signal_fired) == 2.
#[test]
fn issue_121_cooldown_boundary_inclusive() {
    let g = graph(json!({
        "nodes": [
            {"id": "always", "kind": "signal", "subtype": "threshold",
             "config": {"source": {"kind": "price", "symbol": "ETH"},
                        "operator": "<", "reference": {"const": "99999"}}},
            {"id": "buy", "kind": "action", "subtype": "swap",
             "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "1", "chain": "base"}}
        ],
        "edges": [{"from": "always", "to": "buy"}]
    }));
    let mut policy = strict();
    policy.signals = serde_json::from_value(json!({
        "trigger": "level", "repeat": "with_cooldown", "cooldown": "1h"
    }))
    .unwrap();
    let input = SimulationInput {
        graph: g,
        config: config(json!({"base": {"USDC": "1000"}}), 3),
        policy,
        market_data: flat_bundle("base", &["2000", "2000", "2000"]),
    };
    let trace = run(&input).unwrap();
    let fires = count(&trace, "signal_fired");
    assert_eq!(
        fires, 3,
        "issue #121(d): inclusive cooldown boundary (`ts-last < cd`) re-fires at exactly 1h, \
         giving 3 fires (INCORRECT); a strict `<=` guard would give 2"
    );
    assert_ne!(fires, 2, "issue #121(d): the strict-boundary value would be 2");
}

// ---------------------------------------------------------------------------
// (e) CONFIRM-AS-INTENDED: expire_after_bars=1 yields exactly ONE bar of fill
//     eligibility (eligible at placed+1, expired at placed+2). Locked as a
//     regression guard, not a bug.
// ---------------------------------------------------------------------------

/// Issue #121 (e): a good_til_bars order with expire_after_bars=1 has EXACTLY
/// one bar of eligibility. This is the coherent/intended behavior, locked here
/// as a regression guard (confirms the boundary is one bar — not zero, not two).
///
/// Touch case: placed bar0; eligible bar1 (low 1850 <= 1900) -> fills @1900,
/// 0 expirations. No-touch case: same order but lows stay 1990 -> never fills,
/// expires on bar2.
#[test]
fn issue_121_tif_one_bar_eligibility() {
    let node = json!({
        "id": "open", "kind": "action", "subtype": "perp_order",
        "config": {"symbol": "ETH", "side": "long", "size_usd": "500", "leverage": "2",
                   "chain": "hyperliquid", "order_type": "limit", "limit_price": "1900",
                   "reduce_only": false, "time_in_force": "good_til_bars",
                   "expire_after_bars": 1}
    });

    // Touch case: bar1 dips to 1850 -> single eligibility bar fills.
    let g = graph(json!({ "nodes": [node.clone()], "edges": [] }));
    let touch = run(&SimulationInput {
        graph: g,
        config: config(json!({"hyperliquid": {"USDC": "1000"}}), 3),
        policy: frictionless(),
        market_data: ohlc_bundle(
            "hyperliquid",
            &[
                ("2000", "2010", "1990", "2000"),
                ("1980", "1985", "1850", "1900"),
                ("2000", "2010", "1990", "2000"),
            ],
        ),
    })
    .unwrap();
    assert_eq!(count(&touch, "order_filled"), 1, "issue #121(e): the single eligible bar must fill");
    assert_eq!(count(&touch, "order_expired"), 0, "issue #121(e): a filled order does not expire");
    let filled = touch.events.iter().find(|e| e.event_type == "order_filled").unwrap();
    assert_eq!(filled.ts, ts(1), "issue #121(e): fill occurs on the one eligible bar (bar 1)");
    assert_eq!(
        filled.detail.as_ref().unwrap().get("price").unwrap(),
        "1900",
        "issue #121(e): touched limit fills at 1900"
    );

    // No-touch case: lows stay 1990 -> never eligible-to-fill, expires bar2.
    let g2 = graph(json!({ "nodes": [node], "edges": [] }));
    let no_touch = run(&SimulationInput {
        graph: g2,
        config: config(json!({"hyperliquid": {"USDC": "1000"}}), 3),
        policy: frictionless(),
        market_data: ohlc_bundle(
            "hyperliquid",
            &[
                ("2000", "2010", "1990", "2000"),
                ("2000", "2010", "1990", "2000"),
                ("2000", "2010", "1990", "2000"),
            ],
        ),
    })
    .unwrap();
    assert_eq!(count(&no_touch, "order_filled"), 0, "issue #121(e): no touch -> no fill");
    assert_eq!(count(&no_touch, "order_expired"), 1, "issue #121(e): expires after its one bar");
    let expired = no_touch.events.iter().find(|e| e.event_type == "order_expired").unwrap();
    assert_eq!(expired.ts, ts(2), "issue #121(e): expiry on bar 2 (placed 0 + 1 elapsed + 1)");
}
