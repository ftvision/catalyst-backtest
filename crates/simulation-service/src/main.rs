//! Runs the Catalyst simulation service.
//!
//! Bind address defaults to `127.0.0.1:8080`; override with `CATALYST_SIM_BIND`.

use catalyst_simulation_service::app;

#[tokio::main]
async fn main() {
    let bind = std::env::var("CATALYST_SIM_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {bind}: {e}"));
    println!("catalyst-simulation-service listening on http://{bind}");
    axum::serve(listener, app()).await.expect("server error");
}
