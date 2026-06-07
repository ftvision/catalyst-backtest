//! Runs the Catalyst backtest service.
//!
//! - Bind address: `CATALYST_SIM_BIND` (default `127.0.0.1:8080`).
//! - Parquet store root for by-store runs/coverage: `CATALYST_STORE_ROOT`
//!   (local path, `file://`, `s3://...`, `gs://...`); optional.
//! - Worker pool size (queue drainers): `CATALYST_SIM_WORKERS` (default 4).
//! - Job queue capacity: `CATALYST_SIM_QUEUE` (default 1024).
//! - Strategy dataset root: `CATALYST_STRATEGY_ROOT` (default repo `strategies/`).

use catalyst_simulation_service::{app, AppState};

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() {
    let bind = std::env::var("CATALYST_SIM_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let store_root = std::env::var("CATALYST_STORE_ROOT").ok();
    let workers = env_usize("CATALYST_SIM_WORKERS", 4);
    let queue_capacity = env_usize("CATALYST_SIM_QUEUE", 1024);

    let state = AppState::new(store_root, queue_capacity);
    state.start_workers(workers);

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {bind}: {e}"));
    println!("catalyst-backtest-service listening on http://{bind} ({workers} workers)");
    axum::serve(listener, app(state))
        .await
        .expect("server error");
}
