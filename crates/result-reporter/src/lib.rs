//! Turn a raw [`SimulationTrace`] into a user-facing [`BacktestResult`].
//!
//! Pure and deterministic (no I/O): computes the summary, equity/drawdown curves,
//! a trade log, a costs breakdown, and carries the resolved policy and
//! data-provider coverage through unchanged. Rust port of the Python
//! `catalyst_result_reporter` (per ADR 0001 the run path is Rust).

use rust_decimal::Decimal;
use serde_json::Value;

use catalyst_contracts::result::{
    BacktestResult, Costs, DrawdownPoint, EquityPoint, ResultMetadata, Summary, Trade,
};
use catalyst_contracts::trace::Event;
use catalyst_contracts::SimulationTrace;

const EXECUTED: &str = "action_executed";
const REJECTED: &str = "action_rejected";
const LIQUIDATION: &str = "liquidation";
const ORDER_PLACED: &str = "order_placed";
const ORDER_FILLED: &str = "order_filled";
const ORDER_EXPIRED: &str = "order_expired";
const ORDER_REJECTED: &str = "order_rejected";

fn dec(s: &str) -> Decimal {
    s.parse().unwrap_or(Decimal::ZERO)
}

/// Plain decimal string (no exponent, trailing zeros trimmed).
fn fmt(d: Decimal) -> String {
    d.normalize().to_string()
}

/// Read a detail field as a string, tolerating JSON strings *or* numbers.
fn detail_str(detail: &Option<Value>, key: &str) -> Option<String> {
    let v = detail.as_ref()?.get(key)?;
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn detail_dec(detail: &Option<Value>, key: &str) -> Decimal {
    detail_str(detail, key).map(|s| dec(&s)).unwrap_or(Decimal::ZERO)
}

/// Summarize a trace into a [`BacktestResult`]. `data_coverage` is the provider
/// metadata from the market data bundle (carried through verbatim).
pub fn summarize(
    trace: &SimulationTrace,
    data_coverage: Vec<Value>,
    starting_value_usd: Option<&str>,
) -> BacktestResult {
    let equity_curve: Vec<EquityPoint> = trace
        .snapshots
        .iter()
        .map(|s| EquityPoint { ts: s.ts.clone(), equity_usd: s.equity_usd.clone() })
        .collect();

    let start_val = match starting_value_usd {
        Some(s) => dec(s),
        None => trace.snapshots.first().map(|s| dec(&s.equity_usd)).unwrap_or(Decimal::ZERO),
    };
    let final_val = trace.snapshots.last().map(|s| dec(&s.equity_usd)).unwrap_or(start_val);
    let pnl = final_val - start_val;
    let return_pct = if start_val.is_zero() {
        Decimal::ZERO
    } else {
        pnl / start_val * Decimal::from(100)
    };

    let (drawdown_curve, max_drawdown) = drawdown(trace);
    let trades = trades(trace);
    let costs = costs(trace);

    // A filled limit order is a trade, just as a market action is; a rejected one
    // counts alongside rejected market actions.
    let executed = trace
        .events
        .iter()
        .filter(|e| e.event_type == EXECUTED || e.event_type == ORDER_FILLED)
        .count() as u64;
    let rejected = trace
        .events
        .iter()
        .filter(|e| e.event_type == REJECTED || e.event_type == ORDER_REJECTED)
        .count() as u64;

    let summary = Summary {
        starting_value_usd: fmt(start_val),
        final_value_usd: fmt(final_val),
        pnl_usd: fmt(pnl),
        return_pct: fmt(return_pct),
        max_drawdown_pct: Some(fmt(max_drawdown)),
        trade_count: Some(executed),
        rejected_count: Some(rejected),
    };

    let metadata = ResultMetadata {
        policy: trace.policy.clone(),
        interval: Some(trace.interval.clone()),
        start: Some(trace.start.clone()),
        end: Some(trace.end.clone()),
        data_coverage,
        warnings: trace.warnings.clone(),
    };

    BacktestResult {
        schema_version: "catalyst.backtest.result.v1".to_string(),
        summary,
        equity_curve,
        drawdown_curve,
        trades,
        final_portfolio: Some(trace.final_portfolio.clone()),
        costs: Some(costs),
        metadata,
    }
}

fn drawdown(trace: &SimulationTrace) -> (Vec<DrawdownPoint>, Decimal) {
    let mut curve = Vec::new();
    let mut peak = Decimal::ZERO;
    let mut max_dd = Decimal::ZERO;
    for snap in &trace.snapshots {
        let equity = dec(&snap.equity_usd);
        if equity > peak {
            peak = equity;
        }
        let dd = if peak > Decimal::ZERO {
            (equity - peak) / peak * Decimal::from(100)
        } else {
            Decimal::ZERO
        };
        if dd < max_dd {
            max_dd = dd;
        }
        curve.push(DrawdownPoint { ts: snap.ts.clone(), drawdown_pct: fmt(dd) });
    }
    (curve, max_dd)
}

fn trades(trace: &SimulationTrace) -> Vec<Trade> {
    let mut out = Vec::new();
    for event in &trace.events {
        match event.event_type.as_str() {
            EXECUTED | ORDER_FILLED => out.push(executed_trade(event)),
            REJECTED => out.push(status_trade(event, "rejected", "rejected")),
            ORDER_PLACED => out.push(status_trade(event, "limit", "placed")),
            ORDER_EXPIRED => out.push(status_trade(event, "limit", "expired")),
            ORDER_REJECTED => out.push(status_trade(event, "limit", "rejected")),
            LIQUIDATION => out.push(Trade {
                ts: event.ts.clone(),
                node_id: event.node_id.clone().unwrap_or_default(),
                kind: "liquidation".to_string(),
                venue: detail_str(&event.detail, "venue"),
                symbol: detail_str(&event.detail, "symbol"),
                price: detail_str(&event.detail, "mark"),
                status: Some("executed".to_string()),
                reason: event.reason.clone(),
                side: None,
                amount: None,
                value_usd: None,
                fee_usd: None,
                gas_usd: None,
            }),
            _ => {}
        }
    }
    out
}

/// A non-fill lifecycle row (rejected / order placed / expired). Pulls whatever
/// descriptive fields the event detail carries; `limit_price` shows as the price.
fn status_trade(event: &Event, kind: &str, status: &str) -> Trade {
    let d = &event.detail;
    Trade {
        ts: event.ts.clone(),
        node_id: event.node_id.clone().unwrap_or_default(),
        kind: detail_str(d, "kind").unwrap_or_else(|| kind.to_string()),
        status: Some(status.to_string()),
        reason: event.reason.clone(),
        venue: detail_str(d, "venue"),
        symbol: detail_str(d, "symbol"),
        side: detail_str(d, "side"),
        price: detail_str(d, "limit_price"),
        amount: None,
        value_usd: None,
        fee_usd: None,
        gas_usd: None,
    }
}

fn executed_trade(event: &Event) -> Trade {
    let d = &event.detail;
    Trade {
        ts: event.ts.clone(),
        node_id: event.node_id.clone().unwrap_or_default(),
        kind: detail_str(d, "kind").unwrap_or_else(|| "action".to_string()),
        venue: detail_str(d, "venue"),
        symbol: detail_str(d, "symbol"),
        side: detail_str(d, "side"),
        price: detail_str(d, "price"),
        amount: detail_str(d, "amount"),
        value_usd: detail_str(d, "value_usd"),
        fee_usd: detail_str(d, "fee_usd"),
        gas_usd: detail_str(d, "gas_usd"),
        status: Some("executed".to_string()),
        reason: None,
    }
}

fn costs(trace: &SimulationTrace) -> Costs {
    let mut fees = Decimal::ZERO;
    let mut gas = Decimal::ZERO;
    let mut funding = Decimal::ZERO;
    let mut yield_ = Decimal::ZERO;
    for event in &trace.events {
        match event.event_type.as_str() {
            EXECUTED | ORDER_FILLED => {
                fees += detail_dec(&event.detail, "fee_usd");
                gas += detail_dec(&event.detail, "gas_usd");
            }
            "funding_applied" => funding += detail_dec(&event.detail, "payment_usd"),
            "yield_accrued" => yield_ += detail_dec(&event.detail, "interest_usd"),
            _ => {}
        }
    }
    Costs {
        total_fees_usd: Some(fmt(fees)),
        total_gas_usd: Some(fmt(gas)),
        total_funding_usd: Some(fmt(funding)),
        total_yield_usd: Some(fmt(yield_)),
    }
}

pub const CRATE_NAME: &str = "catalyst-result-reporter";
