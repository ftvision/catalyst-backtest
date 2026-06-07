//! Helpers shared by the handlers: hashing, graph summary, coverage rows, policy
//! resolution, and data-requirement conversion.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::{json, Value};

use catalyst_contracts::{Graph, MarketDataBundle, SimulationPolicy};
use catalyst_graph_compiler::CompiledGraph;
use catalyst_market_data_loader::{BundleRef, DataRequirements};
use catalyst_simulation_policies::resolve;

/// Stable short hash of a graph (deterministic across runs) for run grouping.
pub fn graph_hash(graph: &Graph) -> String {
    let canonical = serde_json::to_string(graph).unwrap_or_default();
    let mut h = DefaultHasher::new();
    canonical.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Node/edge counts and enabled signal/action ids for the Run Setup view.
pub fn graph_summary(graph: &Graph, compiled: &CompiledGraph) -> Value {
    json!({
        "node_count": graph.nodes.len(),
        "edge_count": graph.edges.len(),
        "signals": compiled.signals.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
        "actions": compiled.actions.iter().map(|a| a.id.clone()).collect::<Vec<_>>(),
    })
}

/// The default policy selector (strict_v1) when a request omits one.
pub fn default_policy() -> SimulationPolicy {
    SimulationPolicy {
        schema_version: "catalyst.backtest.policy.v1".to_string(),
        profile: "strict_v1".to_string(),
        balance: None,
        fills: None,
        gas: None,
        signals: None,
        ordering: None,
        data: None,
        perps: None,
        yield_: None,
    }
}

/// Resolve a profile name to its full policy JSON (falls back to strict_v1).
pub fn resolved_policy_json(profile: &str) -> Value {
    let resolved = resolve(profile).or_else(|_| resolve("strict_v1")).expect("strict_v1 resolves");
    serde_json::to_value(resolved).unwrap_or(Value::Null)
}

/// The three policy profiles with id/label/description/resolved policy.
pub fn list_profiles() -> Vec<Value> {
    const PROFILES: &[(&str, &str, &str)] = &[
        ("strict_v1", "Strict", "Deterministic correctness: reject insufficient balance, no partial fills, close fills, crossing triggers, fail on missing required data."),
        ("conservative_v1", "Conservative", "Less optimistic: worse-side OHLC fills, higher slippage, adverse same-tick ordering, fallback for optional data."),
        ("research_v1", "Research", "Exploratory: close fills, lower slippage, forward-fill missing data, tolerate fallbacks."),
    ];
    PROFILES
        .iter()
        .map(|(id, label, desc)| {
            json!({ "id": id, "label": label, "description": desc, "resolved_policy": resolved_policy_json(id) })
        })
        .collect()
}

/// Per-series coverage rows + provider metadata + warnings for the coverage view.
pub fn coverage_response(bundle: &MarketDataBundle) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let span = |ts: Option<&String>| ts.cloned();

    for s in &bundle.candles {
        rows.push(json!({"kind": "candles", "venue": s.venue, "symbol": s.symbol,
            "points": s.points.len(), "complete": !s.points.is_empty(),
            "start": span(s.points.first().map(|p| &p.ts)), "end": span(s.points.last().map(|p| &p.ts))}));
    }
    for s in &bundle.funding {
        rows.push(json!({"kind": "funding", "venue": s.venue, "symbol": s.symbol,
            "points": s.points.len(), "complete": !s.points.is_empty(),
            "start": span(s.points.first().map(|p| &p.ts)), "end": span(s.points.last().map(|p| &p.ts))}));
    }
    for s in &bundle.gas {
        rows.push(json!({"kind": "gas", "chain": s.chain,
            "points": s.points.len(), "complete": !s.points.is_empty(),
            "start": span(s.points.first().map(|p| &p.ts)), "end": span(s.points.last().map(|p| &p.ts))}));
    }
    for s in &bundle.yields {
        rows.push(json!({"kind": "yields", "protocol": s.protocol, "asset": s.asset, "chain": s.chain,
            "points": s.points.len(), "complete": !s.points.is_empty(),
            "start": span(s.points.first().map(|p| &p.ts)), "end": span(s.points.last().map(|p| &p.ts))}));
    }

    let providers: Vec<Value> =
        bundle.providers.iter().map(|p| serde_json::to_value(p).unwrap_or(Value::Null)).collect();
    json!({ "coverage": rows, "providers": providers, "warnings": bundle.warnings })
}

/// Provider metadata as JSON values (for the reporter's data_coverage).
pub fn provider_values(bundle: &MarketDataBundle) -> Vec<Value> {
    bundle.providers.iter().map(|p| serde_json::to_value(p).unwrap_or(Value::Null)).collect()
}

/// Build a loader `BundleRef` from a store root + compiled data requirements.
pub fn bundle_ref(root: String, compiled: &CompiledGraph) -> BundleRef {
    let dr: DataRequirements = serde_json::from_value(
        serde_json::to_value(&compiled.data_requirements).unwrap_or(Value::Null),
    )
    .unwrap_or_default();
    BundleRef { root, data_requirements: dr }
}

fn interval_secs(interval: &str) -> Option<i64> {
    Some(match interval {
        "1m" => 60,
        "5m" => 300,
        "15m" => 900,
        "1h" => 3600,
        "4h" => 14_400,
        "1d" => 86_400,
        _ => return None,
    })
}

/// The earliest timestamp to load so derived signals have `lookback_bars` of
/// warmup history before the run's `start`. Returns `start` unchanged when no
/// warmup is needed or the inputs can't be parsed.
pub fn warmup_start(start: &str, interval: &str, lookback_bars: u32) -> String {
    if lookback_bars == 0 {
        return start.to_string();
    }
    let Some(secs) = interval_secs(interval) else { return start.to_string() };
    match chrono::DateTime::parse_from_rfc3339(start) {
        Ok(dt) => {
            let earlier = dt - chrono::Duration::seconds(secs * lookback_bars as i64);
            earlier.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string()
        }
        Err(_) => start.to_string(),
    }
}
