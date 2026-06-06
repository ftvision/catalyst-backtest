//! HTTP wrapper around the Rust simulation engine.
//!
//! Exposes `POST /simulate` (compiled inputs in, simulation trace out) and
//! `GET /health`. The HTTP boundary uses the shared contract types end to end.
//!
//! Market data can be supplied two ways:
//! - **inline** — a fully normalized `market_data` bundle in the request, or
//! - **by reference** — a `market_data_ref` (Parquet store root + data
//!   requirements) that the service reads directly via `catalyst-market-data-loader`,
//!   so bulk candle/funding data never crosses the wire as JSON (issue #29).
//!
//! The service still does no *fetching* — the by-reference path reads a
//! pre-ingested store (issue #30).

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use catalyst_contracts::{BacktestConfig, Graph, MarketDataBundle, SimulationPolicy};
use catalyst_market_data_loader::{load_bundle, BundleRef};
use catalyst_simulation_engine::{run, SimulationInput};

/// Body of `POST /simulate`. Provide exactly one of `market_data` (inline) or
/// `market_data_ref` (read from the Parquet store).
#[derive(Debug, Deserialize)]
pub struct SimulateRequest {
    pub graph: Graph,
    pub config: BacktestConfig,
    #[serde(default = "default_policy")]
    pub policy: SimulationPolicy,
    #[serde(default)]
    pub market_data: Option<MarketDataBundle>,
    #[serde(default)]
    pub market_data_ref: Option<BundleRef>,
}

fn default_policy() -> SimulationPolicy {
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

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    (status, Json(ErrorBody { error: ErrorDetail { code: code.into(), message: message.into() } }))
        .into_response()
}

/// Build the router. Exposed so integration tests can drive it without a socket.
pub fn app() -> Router {
    Router::new().route("/health", get(health)).route("/simulate", post(simulate))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "catalyst-simulation-service" }))
}

async fn simulate(Json(body): Json<serde_json::Value>) -> Response {
    // Deserialize manually so a malformed request yields a structured 400.
    let request: SimulateRequest = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_REQUEST, "invalid_request", e.to_string()),
    };

    // Resolve market data: inline bundle, or load from the Parquet store by ref.
    let market_data = match (request.market_data, &request.market_data_ref) {
        (Some(bundle), None) => bundle,
        (None, Some(reference)) => {
            match load_bundle(
                reference,
                &request.config.start,
                &request.config.end,
                &request.config.interval,
            ) {
                Ok(bundle) => bundle,
                Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, "data_load_error", e.to_string()),
            }
        }
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
