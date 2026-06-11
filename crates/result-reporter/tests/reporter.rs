//! Tests for the Rust result reporter (mirror the Python reporter cases).

use catalyst_contracts::SimulationTrace;
use catalyst_result_reporter::summarize;
use serde_json::{json, Value};

fn trace_from(snapshots: Value, events: Value, final_portfolio: Value) -> SimulationTrace {
    let doc = json!({
        "schema_version": "catalyst.backtest.trace.v1",
        "policy": {"schema_version": "catalyst.backtest.policy.v1", "profile": "strict_v1"},
        "interval": "1h",
        "start": "2024-01-01T00:00:00Z",
        "end": "2024-01-01T02:00:00Z",
        "snapshots": snapshots,
        "events": events,
        "final_portfolio": final_portfolio,
        "warnings": [],
        "errors": []
    });
    serde_json::from_value(doc).unwrap()
}

fn snap(ts: &str, equity: &str) -> Value {
    json!({"ts": ts, "equity_usd": equity})
}

fn empty_portfolio() -> Value {
    json!({"balances": {}, "perp_positions": [], "yield_positions": []})
}

#[test]
fn summary_and_drawdown_math() {
    let trace = trace_from(
        json!([
            snap("2024-01-01T00:00:00Z", "1000"),
            snap("2024-01-01T01:00:00Z", "1200"),
            snap("2024-01-01T02:00:00Z", "900"),
        ]),
        json!([]),
        empty_portfolio(),
    );
    let r = summarize(&trace, vec![], None);
    assert_eq!(r.summary.starting_value_usd, "1000");
    assert_eq!(r.summary.final_value_usd, "900");
    assert_eq!(r.summary.pnl_usd, "-100");
    assert_eq!(r.summary.return_pct, "-10");
    assert_eq!(r.summary.max_drawdown_pct.as_deref(), Some("-25"));
    assert_eq!(r.drawdown_curve.len(), 3);
}

#[test]
fn empty_run_is_zeroed() {
    let trace = trace_from(json!([]), json!([]), empty_portfolio());
    let r = summarize(&trace, vec![], None);
    assert_eq!(r.summary.starting_value_usd, "0");
    assert_eq!(r.summary.final_value_usd, "0");
    assert_eq!(r.summary.pnl_usd, "0");
    assert_eq!(r.summary.return_pct, "0");
    assert!(r.equity_curve.is_empty());
    assert!(r.trades.is_empty());
}

#[test]
fn executed_and_rejected_trades_and_costs() {
    let trace = trace_from(
        json!([snap("2024-01-01T00:00:00Z", "1000")]),
        json!([
            {"ts": "2024-01-01T00:00:00Z", "type": "action_executed", "node_id": "buy",
             "detail": {"kind": "swap", "venue": "base", "symbol": "ETH", "side": "buy",
                        "price": "2002", "amount": "0.05", "value_usd": "100",
                        "fee_usd": "0.05", "gas_usd": "0.02"}},
            {"ts": "2024-01-01T01:00:00Z", "type": "action_rejected", "node_id": "sell",
             "reason": "insufficient balance"}
        ]),
        empty_portfolio(),
    );
    let r = summarize(&trace, vec![], None);
    assert_eq!(r.summary.trade_count, Some(1));
    assert_eq!(r.summary.rejected_count, Some(1));
    let buy = r.trades.iter().find(|t| t.node_id == "buy").unwrap();
    assert_eq!(buy.status.as_deref(), Some("executed"));
    assert_eq!(buy.symbol.as_deref(), Some("ETH"));
    let sell = r.trades.iter().find(|t| t.node_id == "sell").unwrap();
    assert_eq!(sell.status.as_deref(), Some("rejected"));
    let costs = r.costs.unwrap();
    assert_eq!(costs.total_fees_usd.as_deref(), Some("0.05"));
    assert_eq!(costs.total_gas_usd.as_deref(), Some("0.02"));
}

#[test]
fn limit_order_lifecycle_in_trades_and_counts() {
    let trace = trace_from(
        json!([snap("2024-01-01T00:00:00Z", "1000")]),
        json!([
            {"ts": "2024-01-01T00:00:00Z", "type": "order_placed", "node_id": "lim",
             "detail": {"order_id": "lim#0", "kind": "perp", "side": "buy",
                        "limit_price": "1900", "venue": "hyperliquid", "symbol": "ETH"}},
            {"ts": "2024-01-01T01:00:00Z", "type": "order_filled", "node_id": "lim",
             "detail": {"order_id": "lim#0", "kind": "perp_open", "venue": "hyperliquid",
                        "symbol": "ETH", "side": "long", "price": "1900", "amount": "0.26",
                        "value_usd": "500", "fee_usd": "0.25", "gas_usd": "0",
                        "limit_price": "1900"}},
            {"ts": "2024-01-01T02:00:00Z", "type": "order_expired", "node_id": "other",
             "detail": {"order_id": "other#1"}, "reason": "time_in_force elapsed"}
        ]),
        empty_portfolio(),
    );
    let r = summarize(&trace, vec![], None);
    // a filled limit order counts as a trade; placed/expired are lifecycle rows
    assert_eq!(r.summary.trade_count, Some(1));
    let filled = r.trades.iter().find(|t| t.status.as_deref() == Some("executed")).unwrap();
    assert_eq!(filled.kind, "perp_open");
    assert_eq!(filled.price.as_deref(), Some("1900"));
    let placed = r.trades.iter().find(|t| t.status.as_deref() == Some("placed")).unwrap();
    assert_eq!(placed.price.as_deref(), Some("1900")); // shows the limit price
    assert!(r.trades.iter().any(|t| t.status.as_deref() == Some("expired")));
    // the fill's fee rolls into costs
    assert_eq!(r.costs.unwrap().total_fees_usd.as_deref(), Some("0.25"));
}

#[test]
fn liquidation_is_logged() {
    let trace = trace_from(
        json!([snap("2024-01-01T00:00:00Z", "500")]),
        json!([{"ts": "2024-01-01T00:00:00Z", "type": "liquidation",
                "reason": "hyperliquid ETH position liquidated",
                "detail": {"venue": "hyperliquid", "symbol": "ETH", "mark": "1500"}}]),
        empty_portfolio(),
    );
    let r = summarize(&trace, vec![], None);
    let liq: Vec<_> = r.trades.iter().filter(|t| t.kind == "liquidation").collect();
    assert_eq!(liq.len(), 1);
    assert_eq!(liq[0].symbol.as_deref(), Some("ETH"));
    assert!(liq[0].reason.is_some());
}

#[test]
fn funding_and_yield_costs_summed() {
    let trace = trace_from(
        json!([snap("2024-01-01T00:00:00Z", "1000")]),
        json!([
            {"ts": "2024-01-01T00:00:00Z", "type": "funding_applied", "detail": {"payment_usd": "1.5"}},
            // #166: the engine emits asset-unit `interest` alongside the
            // converted `interest_usd`; the reporter must sum the USD field,
            // never the asset units (0.0001 ETH at 3000 -> 0.3 USD).
            {"ts": "2024-01-01T01:00:00Z", "type": "yield_accrued",
             "detail": {"interest": "0.0001", "price": "3000", "interest_usd": "0.3"}}
        ]),
        empty_portfolio(),
    );
    let costs = summarize(&trace, vec![], None).costs.unwrap();
    assert_eq!(costs.total_funding_usd.as_deref(), Some("1.5"));
    assert_eq!(costs.total_yield_usd.as_deref(), Some("0.3"));
}

#[test]
fn funding_costs_prefer_collected_over_owed() {
    // #165: a strict-policy funding shortfall can forgive part of the owed
    // payment at true bankruptcy. The engine reports both; total_funding_usd
    // must sum what actually moved money (`collected_usd`), falling back to
    // `payment_usd` only for traces that predate the field (previous test).
    let trace = trace_from(
        json!([snap("2024-01-01T00:00:00Z", "1000")]),
        json!([
            {"ts": "2024-01-01T00:00:00Z", "type": "funding_applied",
             "detail": {"payment_usd": "150", "collected_usd": "102"}},
            {"ts": "2024-01-01T00:00:00Z", "type": "funding_shortfall",
             "detail": {"payment": "150", "paid_cash": "2", "from_margin": "100", "forgiven": "48"}}
        ]),
        empty_portfolio(),
    );
    let costs = summarize(&trace, vec![], None).costs.unwrap();
    assert_eq!(costs.total_funding_usd.as_deref(), Some("102"));
}

#[test]
fn preserves_policy_and_coverage_and_handles_numeric_detail() {
    let providers = vec![json!({"name": "parquet-store", "kind": "candles"})];
    let trace = trace_from(
        json!([snap("2024-01-01T00:00:00Z", "1000")]),
        // fee_usd as a JSON number (not string) — must still sum
        json!([{"ts": "2024-01-01T00:00:00Z", "type": "action_executed", "node_id": "b",
                "detail": {"kind": "swap", "fee_usd": 0.05}}]),
        empty_portfolio(),
    );
    let r = summarize(&trace, providers.clone(), Some("5000"));
    assert_eq!(r.summary.starting_value_usd, "5000");
    assert_eq!(r.metadata.policy.profile, "strict_v1");
    assert_eq!(r.metadata.data_coverage, providers);
    assert_eq!(r.costs.unwrap().total_fees_usd.as_deref(), Some("0.05"));
}

/// #167: requested vs effective window flows from the trace into the result
/// metadata, so a shortened run is disclosed to the user.
#[test]
fn metadata_carries_requested_and_effective_window() {
    let mut trace = trace_from(
        json!([snap("2024-01-01T01:00:00Z", "1000")]),
        json!([]),
        empty_portfolio(),
    );
    trace.effective_start = Some("2024-01-01T01:00:00Z".to_string());
    trace.effective_end = Some("2024-01-01T02:00:00Z".to_string());
    let r = summarize(&trace, vec![], None);
    assert_eq!(r.metadata.start.as_deref(), Some("2024-01-01T00:00:00Z"));
    assert_eq!(r.metadata.end.as_deref(), Some("2024-01-01T02:00:00Z"));
    assert_eq!(r.metadata.effective_start.as_deref(), Some("2024-01-01T01:00:00Z"));
    assert_eq!(r.metadata.effective_end.as_deref(), Some("2024-01-01T02:00:00Z"));
}
