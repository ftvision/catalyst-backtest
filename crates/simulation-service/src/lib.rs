//! The Catalyst backtest service (HTTP).
//!
//! Per [ADR 0001] the deterministic run path is Rust; this service is the
//! user-facing API. It orchestrates the run **in-process** — compile (graph),
//! resolve policy, load/accept market data, run the engine, summarize — using the
//! Rust crates directly (no internal HTTP hop), and serves both the run lifecycle
//! and the workbench-setup endpoints.
//!
//! Endpoints:
//! - `GET  /health`
//! - `POST /simulate` — low-level: inputs in, raw `SimulationTrace` out
//! - `POST /backtests` — run + persist; `GET /backtests` — history (`?graph_hash=`)
//! - `GET  /backtests/{id}` `/result` `/metadata` `/events` (paginated/filterable)
//! - `POST /backtests/preview` — validate graph + summary + data requirements + resolved policy
//! - `POST /market-data/coverage` — per-series coverage before a run
//! - `POST /market-data/window` — normalized bundle for the requested window
//! - `GET  /policy-profiles`
//! - `GET  /strategies` `/strategies/{id}` — bundled strategy repository
//! - `GET  /strategy-scenarios` `/strategy-scenarios/{id}` — bundled market scenarios
//!
//! Market data is either inline in the request or read from the configured
//! Parquet store (`AppState::store_root`) via `catalyst-market-data-loader`.
//!
//! [ADR 0001]: ../../../docs/adr/0001-language-boundary.md

mod error;
mod handlers;
mod state;
mod support;
mod worker;

pub use state::AppState;

use axum::{
    http::Method,
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

/// Build the router with injected state. Exposed so tests can drive it without a socket.
pub fn app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    Router::new()
        .route("/health", get(handlers::health))
        .route("/simulate", post(handlers::simulate))
        .route(
            "/backtests",
            post(handlers::create_backtest).get(handlers::list_backtests),
        )
        .route("/backtests/preview", post(handlers::preview))
        .route("/backtests/:id", get(handlers::get_backtest))
        .route("/backtests/:id/result", get(handlers::get_result))
        .route("/backtests/:id/metadata", get(handlers::get_metadata))
        .route("/backtests/:id/events", get(handlers::get_events))
        .route("/market-data/coverage", post(handlers::coverage))
        .route("/market-data/window", post(handlers::market_data_window))
        .route("/policy-profiles", get(handlers::policy_profiles))
        .route("/strategies", get(handlers::list_strategies))
        .route("/strategies/:id", get(handlers::get_strategy))
        .route(
            "/strategy-scenarios",
            get(handlers::list_strategy_scenarios),
        )
        .route(
            "/strategy-scenarios/:id",
            get(handlers::get_strategy_scenario),
        )
        .with_state(state)
        .layer(cors)
}
