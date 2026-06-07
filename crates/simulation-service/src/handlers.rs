//! HTTP handlers: async run lifecycle + workbench setup + low-level simulate.

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
use catalyst_simulation_engine::{run, SimulationInput};

use crate::error::{error, error_with};
use crate::state::{AppState, Job, StoredRequest, SubmitError};
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

// --- run lifecycle (async) ---

/// Enqueue a backtest. Returns immediately; a worker runs it. (202 Accepted)
pub async fn create_backtest(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let req: BacktestRequestBody = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request", e.to_string()),
    };
    let stored = StoredRequest {
        graph: req.graph,
        config: req.config,
        policy: req.policy,
        market_data: req.market_data,
    };
    let id = state.next_run_id();
    let job = Job::queued(id.clone(), stored);
    match state.submit(job) {
        Ok(()) => {
            (StatusCode::ACCEPTED, Json(json!({ "id": id, "status": "queued" }))).into_response()
        }
        Err(SubmitError::QueueFull) => {
            error(StatusCode::SERVICE_UNAVAILABLE, "queue_full", "backtest queue is full; retry later")
        }
    }
}

pub async fn list_backtests(State(state): State<AppState>, Query(q): Query<ListQuery>) -> Json<Value> {
    let items: Vec<Value> = state
        .list(q.graph_hash.as_deref())
        .into_iter()
        .map(|j| {
            json!({
                "id": j.id, "graph_hash": j.graph_hash, "status": j.status,
                "policy_profile": j.policy_profile, "start": j.start, "end": j.end,
                "interval": j.interval, "created_at": j.created_at,
                "summary": {
                    "final_value_usd": j.summary.get("final_value_usd"),
                    "return_pct": j.summary.get("return_pct"),
                    "max_drawdown_pct": j.summary.get("max_drawdown_pct"),
                },
                "warning_count": j.warnings.len(),
            })
        })
        .collect();
    Json(json!({ "items": items }))
}

pub async fn get_backtest(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.get(&id) {
        Some(j) => Json(json!({
            "id": j.id, "status": j.status, "error": j.error,
            "created_at": j.created_at, "started_at": j.started_at, "finished_at": j.finished_at,
        }))
        .into_response(),
        None => not_found(&id),
    }
}

pub async fn get_result(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(j) = state.get(&id) else { return not_found(&id) };
    match j.status.as_str() {
        "succeeded" => match j.result {
            Some(result) => Json(result).into_response(),
            None => error(StatusCode::CONFLICT, "no_result", format!("backtest {id:?} has no result")),
        },
        "failed" => error(StatusCode::UNPROCESSABLE_ENTITY, "backtest_failed", j.error.unwrap_or_default()),
        other => error_with(
            StatusCode::CONFLICT,
            "not_ready",
            "backtest is not finished",
            json!({ "status": other }),
        ),
    }
}

pub async fn get_metadata(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.get(&id) {
        Some(j) => Json(json!({
            "id": j.id, "graph_hash": j.graph_hash, "status": j.status,
            "created_at": j.created_at, "started_at": j.started_at, "finished_at": j.finished_at,
            "config": { "start": j.start, "end": j.end, "interval": j.interval },
            "resolved_policy": support::resolved_policy_json(&j.policy_profile),
            "data_coverage": j.data_coverage, "warnings": j.warnings, "summary": j.summary,
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
    let Some(job) = state.get(&id) else { return not_found(&id) };
    let events = job
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

// --- workbench setup (synchronous; cheap) ---

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
    let bundle = match load_market_data_for_window(&state, req).await {
        Ok(bundle) => bundle,
        Err(response) => return response,
    };
    Json(support::coverage_response(&bundle)).into_response()
}

pub async fn market_data_window(State(state): State<AppState>, Json(body): Json<Value>) -> Response {
    let req: CoverageBody = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request", e.to_string()),
    };
    match load_market_data_for_window(&state, req).await {
        Ok(bundle) => Json(bundle).into_response(),
        Err(response) => response,
    }
}

pub async fn policy_profiles() -> Json<Value> {
    Json(json!({ "items": support::list_profiles() }))
}

fn not_found(id: &str) -> Response {
    error(StatusCode::NOT_FOUND, "not_found", format!("no backtest {id:?}"))
}

async fn load_market_data_for_window(
    state: &AppState,
    req: CoverageBody,
) -> Result<MarketDataBundle, Response> {
    let compiled = compile(&req.graph)
        .map_err(|e| error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_graph", e.to_string()))?;
    match req.market_data {
        Some(bundle) => Ok(bundle),
        None => match state.store_root() {
            Some(root) => {
                let reference: BundleRef = support::bundle_ref(root, &compiled);
                load_bundle(&reference, &req.start, &req.end, &req.interval)
                    .await
                    .map_err(|e| error(StatusCode::UNPROCESSABLE_ENTITY, "data_load_error", e.to_string()))
            }
            None => Err(error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "no market_data supplied and no store configured",
            )),
        },
    }
}

// --- low-level: run the engine and return the raw trace (CPU off the async pool) ---

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
    match tokio::task::spawn_blocking(move || run(&input)).await {
        Ok(Ok(trace)) => (StatusCode::OK, Json(trace)).into_response(),
        Ok(Err(e)) => error(StatusCode::UNPROCESSABLE_ENTITY, "simulation_error", e.to_string()),
        Err(join) => error(StatusCode::INTERNAL_SERVER_ERROR, "internal", join.to_string()),
    }
}
