//! HTTP handlers for the backtest service: run lifecycle + workbench setup.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_graph_compiler::compile;
use catalyst_market_data_loader::{load_bundle, BundleRef};
use catalyst_result_reporter::summarize;
use catalyst_simulation_engine::{run, SimulationInput};

use crate::error::{error, error_with};
use crate::state::{AppState, RunRecord};
use crate::support;

// --- request bodies ---

#[derive(Debug, Deserialize)]
struct BacktestRequestBody {
    graph: Graph,
    config: BacktestConfig,
    #[serde(default = "support::default_policy")]
    policy: SimulationPolicy,
    #[serde(default)]
    market_data: Option<MarketDataBundle>,
}

#[derive(Debug, Deserialize)]
struct PreviewBody {
    graph: Graph,
    #[serde(default = "support::default_policy")]
    policy: SimulationPolicy,
}

#[derive(Debug, Deserialize)]
struct CoverageBody {
    graph: Graph,
    start: String,
    end: String,
    interval: String,
    #[serde(default)]
    market_data: Option<MarketDataBundle>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    graph_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EventQuery {
    #[serde(rename = "type")]
    event_type: Option<String>,
    node_id: Option<String>,
    status: Option<String>,
    #[serde(default)]
    cursor: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    100
}

// --- health ---

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "catalyst-backtest-service" }))
}

// --- run lifecycle ---

pub async fn create_backtest(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let req: BacktestRequestBody = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request", e.to_string()),
    };

    let id = state.next_run_id();
    let gh = support::graph_hash(&req.graph);
    let created = chrono::Utc::now().to_rfc3339();
    let cfg = (req.config.start.clone(), req.config.end.clone(), req.config.interval.clone());
    let profile = req.policy.profile.clone();

    let fail = |code: &str, msg: String, http: StatusCode| -> Response {
        state.insert(RunRecord {
            id: id.clone(),
            graph_hash: gh.clone(),
            status: "failed".into(),
            error: Some(msg.clone()),
            policy_profile: profile.clone(),
            start: cfg.0.clone(),
            end: cfg.1.clone(),
            interval: cfg.2.clone(),
            created_at: created.clone(),
            summary: Value::Null,
            result: None,
            trace: None,
            data_coverage: vec![],
            warnings: vec![],
        });
        error_with(http, code, msg, json!({ "id": id }))
    };

    let compiled = match compile(&req.graph) {
        Ok(c) => c,
        Err(e) => return fail("backtest_failed", e.to_string(), StatusCode::UNPROCESSABLE_ENTITY),
    };

    let bundle = match req.market_data {
        Some(b) => b,
        None => match &state.store_root {
            Some(root) => {
                let r = support::bundle_ref(root.clone(), &compiled);
                match load_bundle(&r, &req.config.start, &req.config.end, &req.config.interval).await {
                    Ok(b) => b,
                    Err(e) => {
                        return fail("data_load_error", e.to_string(), StatusCode::UNPROCESSABLE_ENTITY)
                    }
                }
            }
            None => {
                return fail(
                    "invalid_request",
                    "no market_data supplied and no store configured".into(),
                    StatusCode::BAD_REQUEST,
                )
            }
        },
    };

    let providers = support::provider_values(&bundle);
    let input = SimulationInput {
        graph: req.graph.clone(),
        config: req.config.clone(),
        policy: req.policy.clone(),
        market_data: bundle,
    };
    let trace = match run(&input) {
        Ok(t) => t,
        Err(e) => return fail("simulation_error", e.to_string(), StatusCode::UNPROCESSABLE_ENTITY),
    };

    let result = summarize(&trace, providers.clone(), None);
    let result_json = serde_json::to_value(&result).unwrap_or(Value::Null);
    let summary = result_json.get("summary").cloned().unwrap_or(Value::Null);

    state.insert(RunRecord {
        id: id.clone(),
        graph_hash: gh,
        status: "succeeded".into(),
        error: None,
        policy_profile: profile,
        start: cfg.0,
        end: cfg.1,
        interval: cfg.2,
        created_at: created,
        summary,
        result: Some(result_json),
        trace: serde_json::to_value(&trace).ok(),
        data_coverage: providers,
        warnings: trace.warnings.clone(),
    });

    (StatusCode::CREATED, Json(json!({ "id": id, "status": "succeeded" }))).into_response()
}

pub async fn list_backtests(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    let items: Vec<Value> = state
        .list(q.graph_hash.as_deref())
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id, "graph_hash": r.graph_hash, "status": r.status,
                "policy_profile": r.policy_profile, "start": r.start, "end": r.end,
                "interval": r.interval, "created_at": r.created_at,
                "summary": {
                    "final_value_usd": r.summary.get("final_value_usd"),
                    "return_pct": r.summary.get("return_pct"),
                    "max_drawdown_pct": r.summary.get("max_drawdown_pct"),
                },
                "warning_count": r.warnings.len(),
            })
        })
        .collect();
    Json(json!({ "items": items }))
}

pub async fn get_backtest(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.get(&id) {
        Some(r) => Json(json!({ "id": r.id, "status": r.status, "error": r.error })).into_response(),
        None => not_found(&id),
    }
}

pub async fn get_result(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.get(&id) {
        Some(r) => match r.result {
            Some(result) => Json(result).into_response(),
            None => error(StatusCode::CONFLICT, "no_result", format!("backtest {id:?} has no result")),
        },
        None => not_found(&id),
    }
}

pub async fn get_metadata(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.get(&id) {
        Some(r) => Json(json!({
            "id": r.id, "graph_hash": r.graph_hash, "status": r.status, "created_at": r.created_at,
            "config": { "start": r.start, "end": r.end, "interval": r.interval },
            "resolved_policy": support::resolved_policy_json(&r.policy_profile),
            "data_coverage": r.data_coverage, "warnings": r.warnings, "summary": r.summary,
        }))
        .into_response(),
        None => not_found(&id),
    }
}

pub async fn get_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<EventQuery>,
) -> Response {
    let record = match state.get(&id) {
        Some(r) => r,
        None => return not_found(&id),
    };
    let events = record
        .trace
        .as_ref()
        .and_then(|t| t.get("events"))
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();

    let status_type = match q.status.as_deref() {
        Some("executed") => Some("action_executed"),
        Some("rejected") => Some("action_rejected"),
        _ => None,
    };
    let wanted_type = q.event_type.as_deref().or(status_type);

    let filtered: Vec<Value> = events
        .into_iter()
        .filter(|e| {
            wanted_type.map(|t| e.get("type").and_then(|v| v.as_str()) == Some(t)).unwrap_or(true)
                && q
                    .node_id
                    .as_deref()
                    .map(|n| e.get("node_id").and_then(|v| v.as_str()) == Some(n))
                    .unwrap_or(true)
        })
        .collect();

    let total = filtered.len();
    let page: Vec<Value> = filtered.into_iter().skip(q.cursor).take(q.limit).collect();
    let next = if q.cursor + q.limit < total { Some(q.cursor + q.limit) } else { None };
    Json(json!({ "items": page, "next_cursor": next, "total": total })).into_response()
}

// --- workbench setup ---

pub async fn preview(Json(body): Json<Value>) -> Response {
    let req: PreviewBody = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request", e.to_string()),
    };
    let gh = support::graph_hash(&req.graph);
    let resolved = support::resolved_policy_json(&req.policy.profile);
    match compile(&req.graph) {
        Ok(c) => Json(json!({
            "graph_hash": gh, "valid": true,
            "graph_summary": support::graph_summary(&req.graph, &c),
            "data_requirements": serde_json::to_value(&c.data_requirements).unwrap_or(Value::Null),
            "resolved_policy": resolved, "warnings": c.warnings,
        }))
        .into_response(),
        Err(e) => Json(json!({
            "graph_hash": gh, "valid": false, "error": e.to_string(),
            "resolved_policy": resolved, "warnings": [],
        }))
        .into_response(),
    }
}

pub async fn coverage(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let req: CoverageBody = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request", e.to_string()),
    };
    let compiled = match compile(&req.graph) {
        Ok(c) => c,
        Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_graph", e.to_string()),
    };
    let bundle = match req.market_data {
        Some(b) => b,
        None => match &state.store_root {
            Some(root) => {
                let r: BundleRef = support::bundle_ref(root.clone(), &compiled);
                match load_bundle(&r, &req.start, &req.end, &req.interval).await {
                    Ok(b) => b,
                    Err(e) => {
                        return error(StatusCode::UNPROCESSABLE_ENTITY, "data_load_error", e.to_string())
                    }
                }
            }
            None => {
                return error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "no market_data supplied and no store configured",
                )
            }
        },
    };
    Json(support::coverage_response(&bundle)).into_response()
}

pub async fn policy_profiles() -> Json<Value> {
    Json(json!({ "items": support::list_profiles() }))
}

fn not_found(id: &str) -> Response {
    error(StatusCode::NOT_FOUND, "not_found", format!("no backtest {id:?}"))
}

// --- low-level: run the engine and return the raw trace ---

#[derive(Debug, Deserialize)]
struct SimulateRequest {
    graph: Graph,
    config: BacktestConfig,
    #[serde(default = "support::default_policy")]
    policy: SimulationPolicy,
    #[serde(default)]
    market_data: Option<MarketDataBundle>,
    #[serde(default)]
    market_data_ref: Option<BundleRef>,
}

pub async fn simulate(Json(body): Json<Value>) -> Response {
    let request: SimulateRequest = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request", e.to_string()),
    };

    let market_data = match (request.market_data, &request.market_data_ref) {
        (Some(bundle), None) => bundle,
        (None, Some(reference)) => match load_bundle(
            reference,
            &request.config.start,
            &request.config.end,
            &request.config.interval,
        )
        .await
        {
            Ok(bundle) => bundle,
            Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, "data_load_error", e.to_string()),
        },
        (Some(_), Some(_)) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "provide exactly one of market_data or market_data_ref, not both",
            )
        }
        (None, None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "missing market data: provide market_data or market_data_ref",
            )
        }
    };

    let input = SimulationInput {
        graph: request.graph,
        config: request.config,
        policy: request.policy,
        market_data,
    };
    match run(&input) {
        Ok(trace) => (StatusCode::OK, Json(trace)).into_response(),
        Err(e) => error(StatusCode::UNPROCESSABLE_ENTITY, "simulation_error", e.to_string()),
    }
}
