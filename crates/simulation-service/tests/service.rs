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

// --- market_data_ref: the service reads the Parquet store directly (#29) ---

fn write_eth_candles(root: &std::path::Path) {
    use std::sync::Arc;

    use arrow::array::{ArrayRef, StringArray, TimestampMicrosecondArray};
    use arrow::datatypes::{Field, Schema};
    use arrow::record_batch::RecordBatch;
    use parquet::arrow::ArrowWriter;

    let dir = root
        .join("candles")
        .join("venue=base")
        .join("symbol=ETH")
        .join("interval=1h");
    std::fs::create_dir_all(&dir).unwrap();

    let h0: i64 = 1_704_067_200_000_000; // 2024-01-01T00:00:00Z micros
    let hour = 3_600_000_000;
    let ts: ArrayRef =
        Arc::new(TimestampMicrosecondArray::from(vec![h0, h0 + hour]).with_timezone("UTC"));
    let price: ArrayRef = Arc::new(StringArray::from(vec!["2000", "2000"]));
    let cols: Vec<(&str, ArrayRef)> = vec![
        ("ts", ts),
        ("open", price.clone()),
        ("high", price.clone()),
        ("low", price.clone()),
        ("close", price.clone()),
        ("volume", Arc::new(StringArray::from(vec!["1", "1"]))),
    ];
    let fields: Vec<Field> =
        cols.iter().map(|(n, a)| Field::new(*n, a.data_type().clone(), true)).collect();
    let schema = Arc::new(Schema::new(fields));
    let batch =
        RecordBatch::try_new(schema.clone(), cols.iter().map(|(_, a)| a.clone()).collect()).unwrap();
    let file = std::fs::File::create(dir.join("2024-01-01.parquet")).unwrap();
    let mut w = ArrowWriter::try_new(file, schema, None).unwrap();
    w.write(&batch).unwrap();
    w.close().unwrap();
}

#[tokio::test]
async fn simulate_by_reference_reads_parquet_store() {
    let tmp = tempfile::tempdir().unwrap();
    write_eth_candles(tmp.path());

    let req = json!({
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
        // no inline market_data — reference the store instead
        "market_data_ref": {
            "root": tmp.path().to_string_lossy(),
            "data_requirements": { "candles": [{"venue": "base", "symbol": "ETH"}] }
        }
    });

    let (status, value) = post_json("/simulate", req).await;
    assert_eq!(status, StatusCode::OK, "body: {value}");
    assert_eq!(value["snapshots"].as_array().unwrap().len(), 2);
    let executed = value["events"].as_array().unwrap().iter().any(|e| e["type"] == "action_executed");
    assert!(executed, "the swap should execute off the Parquet-loaded prices");
}

#[tokio::test]
async fn both_inline_and_ref_is_400() {
    let mut req = simulate_request(); // has inline market_data
    req["market_data_ref"] = json!({"root": "/tmp/x", "data_requirements": {}});
    let (status, value) = post_json("/simulate", req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn neither_inline_nor_ref_is_400() {
    let mut req = simulate_request();
    req.as_object_mut().unwrap().remove("market_data");
    let (status, value) = post_json("/simulate", req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "invalid_request");
}
