//! Runs the Catalyst backtest service.
//!
//! - Bind address: `CATALYST_SIM_BIND` (default `127.0.0.1:8080`).
//! - Parquet store root for by-store runs/coverage: `CATALYST_STORE_ROOT`
//!   (local path, `file://`, `s3://...`, `gs://...`); optional.

use catalyst_simulation_service::{app, AppState};

#[tokio::main]
async fn main() {
    let bind = std::env::var("CATALYST_SIM_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let store_root = std::env::var("CATALYST_STORE_ROOT").ok();
    let state = AppState::new(store_root);

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {bind}: {e}"));
    println!("catalyst-backtest-service listening on http://{bind}");
    axum::serve(listener, app(state)).await.expect("server error");
}
