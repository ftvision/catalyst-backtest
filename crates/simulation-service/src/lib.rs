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
//! - `GET  /policy-profiles`
//!
//! Market data is either inline in the request or read from the configured
//! Parquet store (`AppState::store_root`) via `catalyst-market-data-loader`.
//!
//! [ADR 0001]: ../../../docs/adr/0001-language-boundary.md

mod error;
mod handlers;
mod state;
mod support;

pub use state::AppState;

use axum::{
    routing::{get, post},
    Router,
};

/// Build the router with injected state. Exposed so tests can drive it without a socket.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/simulate", post(handlers::simulate))
        .route("/backtests", post(handlers::create_backtest).get(handlers::list_backtests))
        .route("/backtests/preview", post(handlers::preview))
        .route("/backtests/:id", get(handlers::get_backtest))
        .route("/backtests/:id/result", get(handlers::get_result))
        .route("/backtests/:id/metadata", get(handlers::get_metadata))
        .route("/backtests/:id/events", get(handlers::get_events))
        .route("/market-data/coverage", post(handlers::coverage))
        .route("/policy-profiles", get(handlers::policy_profiles))
        .with_state(state)
}
