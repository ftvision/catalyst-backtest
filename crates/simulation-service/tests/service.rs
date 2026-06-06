//! HTTP-level tests for the simulation service (no socket; via tower oneshot).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use catalyst_simulation_service::app;

fn simulate_request() -> Value {
    json!({
        "graph": {
            "nodes": [{
                "id": "buy", "kind": "action", "subtype": "swap",
                "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base"}
            }],
            "edges": []
        },
        "config": {
            "start": "2024-01-01T00:00:00Z",
            "end": "2024-01-01T02:00:00Z",
            "interval": "1h",
            "initial_portfolio": {"base": {"USDC": "1000"}}
        },
        "policy": {"profile": "strict_v1"},
        "market_data": {
            "schema_version": "catalyst.backtest.market_data_bundle.v1",
            "interval": "1h",
            "start": "2024-01-01T00:00:00Z",
            "end": "2024-01-01T02:00:00Z",
            "candles": [{
                "venue": "base", "symbol": "ETH", "quote": "USD",
                "points": [
                    {"ts": "2024-01-01T00:00:00Z", "open": "2000", "high": "2000", "low": "2000", "close": "2000"},
                    {"ts": "2024-01-01T01:00:00Z", "open": "2000", "high": "2000", "low": "2000", "close": "2000"}
                ]
            }],
            "gas": [{"chain": "base", "points": [{"ts": "2024-01-01T00:00:00Z", "gas_usd": "0.02"}]}]
        }
    })
}

async fn post_json(uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    (status, value)
}

#[tokio::test]
async fn health_returns_ok() {
    let response =
        app().oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["status"], "ok");
}

#[tokio::test]
async fn simulate_returns_a_trace() {
    let (status, value) = post_json("/simulate", simulate_request()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["policy"]["profile"], "strict_v1");
    assert_eq!(value["snapshots"].as_array().unwrap().len(), 2);
    // the buy executed
    let executed = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["type"] == "action_executed");
    assert!(executed);
    // response is a valid contract trace
    let _trace: catalyst_contracts::SimulationTrace =
        serde_json::from_value(value).unwrap();
}

#[tokio::test]
async fn malformed_request_is_a_structured_400() {
    let (status, value) = post_json("/simulate", json!({"graph": {"nodes": []}})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "invalid_request");
    assert!(value["error"]["message"].is_string());
}

#[tokio::test]
async fn engine_error_is_a_structured_422() {
    let mut req = simulate_request();
    req["config"]["interval"] = json!("3w"); // unsupported interval
    let (status, value) = post_json("/simulate", req).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(value["error"]["code"], "simulation_error");
}
