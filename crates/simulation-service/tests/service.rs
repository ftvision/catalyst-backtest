//! HTTP-level tests for the backtest service (no socket; via tower oneshot).

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use catalyst_simulation_service::{app, AppState};

fn graph() -> Value {
    json!({
        "nodes": [{
            "id": "buy", "kind": "action", "subtype": "swap",
            "config": {"from_asset": "USDC", "to_asset": "ETH", "amount": "100", "chain": "base"}
        }],
        "edges": []
    })
}

fn inline_market_data() -> Value {
    json!({
        "schema_version": "catalyst.backtest.market_data_bundle.v1",
        "interval": "1h", "start": "2024-01-01T00:00:00Z", "end": "2024-01-01T02:00:00Z",
        "candles": [{"venue": "base", "symbol": "ETH", "quote": "USD", "points": [
            {"ts": "2024-01-01T00:00:00Z", "open": "2000", "high": "2000", "low": "2000", "close": "2000"},
            {"ts": "2024-01-01T01:00:00Z", "open": "2000", "high": "2000", "low": "2000", "close": "2000"}
        ]}],
        "gas": [{"chain": "base", "points": [{"ts": "2024-01-01T00:00:00Z", "gas_usd": "0.02"}]}]
    })
}

fn config() -> Value {
    json!({"start": "2024-01-01T00:00:00Z", "end": "2024-01-01T02:00:00Z",
           "interval": "1h", "initial_portfolio": {"base": {"USDC": "1000"}}})
}

fn backtest_body() -> Value {
    json!({"graph": graph(), "config": config(), "policy": {"profile": "strict_v1"},
           "market_data": inline_market_data()})
}

async fn send(state: &AppState, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let req = match body {
        Some(b) => builder
            .header("content-type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = app(state.clone()).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn state() -> AppState {
    AppState::default()
}

// --- health + low-level simulate ---

#[tokio::test]
async fn health_ok() {
    let (s, v) = send(&state(), "GET", "/health", None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["status"], "ok");
}

#[tokio::test]
async fn simulate_returns_trace() {
    let body = json!({"graph": graph(), "config": config(), "policy": {"profile": "strict_v1"},
                      "market_data": inline_market_data()});
    let (s, v) = send(&state(), "POST", "/simulate", Some(body)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["snapshots"].as_array().unwrap().len(), 2);
}

// --- run lifecycle ---

#[tokio::test]
async fn create_then_inspect_lifecycle() {
    let st = state();
    let (s, created) = send(&st, "POST", "/backtests", Some(backtest_body())).await;
    assert_eq!(s, StatusCode::CREATED, "body: {created}");
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["status"], "succeeded");

    let (s, status) = send(&st, "GET", &format!("/backtests/{id}"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(status["status"], "succeeded");

    let (s, result) = send(&st, "GET", &format!("/backtests/{id}/result"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(result.get("summary").is_some());
    assert_eq!(result["metadata"]["policy"]["profile"], "strict_v1");

    let (s, events) = send(&st, "GET", &format!("/backtests/{id}/events"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(events["items"].as_array().unwrap().iter().any(|e| e["type"] == "action_executed"));

    let (s, meta) = send(&st, "GET", &format!("/backtests/{id}/metadata"), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(meta["resolved_policy"]["profile"], "strict_v1");
    assert_eq!(meta["config"]["interval"], "1h");
}

#[tokio::test]
async fn invalid_graph_creates_failed_run() {
    let st = state();
    let mut body = backtest_body();
    body["graph"]["edges"] = json!([{"from": "buy", "to": "ghost"}]);
    let (s, v) = send(&st, "POST", "/backtests", Some(body)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["error"]["code"], "backtest_failed");
    let id = v["id"].as_str().unwrap();
    let (s2, status) = send(&st, "GET", &format!("/backtests/{id}"), None).await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(status["status"], "failed");
}

#[tokio::test]
async fn run_history_by_graph_hash() {
    let st = state();
    send(&st, "POST", "/backtests", Some(backtest_body())).await;
    send(&st, "POST", "/backtests", Some(backtest_body())).await;
    let (_, prev) = send(&st, "POST", "/backtests/preview", Some(json!({"graph": graph()}))).await;
    let gh = prev["graph_hash"].as_str().unwrap();

    let (s, list) = send(&st, "GET", &format!("/backtests?graph_hash={gh}"), None).await;
    assert_eq!(s, StatusCode::OK);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i["graph_hash"] == gh));
    assert!(items[0]["summary"]["final_value_usd"].is_string());
}

#[tokio::test]
async fn events_filter_and_paginate() {
    let st = state();
    let (_, created) = send(&st, "POST", "/backtests", Some(backtest_body())).await;
    let id = created["id"].as_str().unwrap().to_string();

    let (_, all) = send(&st, "GET", &format!("/backtests/{id}/events"), None).await;
    assert!(all["total"].as_u64().unwrap() >= 1);

    let (_, exec) = send(&st, "GET", &format!("/backtests/{id}/events?status=executed"), None).await;
    assert!(exec["items"].as_array().unwrap().iter().all(|e| e["type"] == "action_executed"));

    let (_, page) = send(&st, "GET", &format!("/backtests/{id}/events?limit=1"), None).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn unknown_run_is_404() {
    let st = state();
    for path in [
        "/backtests/nope",
        "/backtests/nope/result",
        "/backtests/nope/metadata",
        "/backtests/nope/events",
    ] {
        let (s, _) = send(&st, "GET", path, None).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
}

// --- workbench setup ---

#[tokio::test]
async fn policy_profiles_lists_three() {
    let (s, v) = send(&state(), "GET", "/policy-profiles", None).await;
    assert_eq!(s, StatusCode::OK);
    let items = v["items"].as_array().unwrap();
    let ids: Vec<&str> = items.iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"strict_v1") && ids.contains(&"conservative_v1") && ids.contains(&"research_v1"));
    let strict = items.iter().find(|p| p["id"] == "strict_v1").unwrap();
    assert_eq!(strict["resolved_policy"]["price_selection"], "close");
}

#[tokio::test]
async fn preview_valid_and_invalid() {
    let st = state();
    let (s, v) = send(
        &st,
        "POST",
        "/backtests/preview",
        Some(json!({"graph": graph(), "policy": {"profile": "conservative_v1"}})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["valid"], true);
    assert_eq!(v["graph_summary"]["actions"][0], "buy");
    assert_eq!(v["resolved_policy"]["price_selection"], "worse_side_ohlc");

    let mut bad = json!({"graph": graph()});
    bad["graph"]["edges"] = json!([{"from": "buy", "to": "ghost"}]);
    let (s2, v2) = send(&st, "POST", "/backtests/preview", Some(bad)).await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(v2["valid"], false);
    assert!(v2["error"].as_str().unwrap().contains("ghost"));
}

#[tokio::test]
async fn coverage_from_inline_bundle() {
    let body = json!({"graph": graph(), "start": "2024-01-01T00:00:00Z", "end": "2024-01-01T02:00:00Z",
                      "interval": "1h", "market_data": inline_market_data()});
    let (s, v) = send(&state(), "POST", "/market-data/coverage", Some(body)).await;
    assert_eq!(s, StatusCode::OK);
    let kinds: Vec<&str> =
        v["coverage"].as_array().unwrap().iter().map(|r| r["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"candles") && kinds.contains(&"gas"));
}

// --- by-reference run reads the configured Parquet store ---

fn write_eth_candles(root: &Path) {
    use arrow::array::{ArrayRef, StringArray, TimestampMicrosecondArray};
    use arrow::datatypes::{Field, Schema};
    use arrow::record_batch::RecordBatch;
    use parquet::arrow::ArrowWriter;

    let dir = root.join("candles").join("venue=base").join("symbol=ETH").join("interval=1h");
    std::fs::create_dir_all(&dir).unwrap();
    let h0: i64 = 1_704_067_200_000_000;
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
async fn create_backtest_reads_configured_store() {
    let tmp = tempfile::tempdir().unwrap();
    write_eth_candles(tmp.path());
    let st = AppState::new(Some(tmp.path().to_string_lossy().to_string()));

    // no inline market_data -> the service loads from the configured store
    let body = json!({"graph": graph(), "config": config(), "policy": {"profile": "strict_v1"}});
    let (s, created) = send(&st, "POST", "/backtests", Some(body)).await;
    assert_eq!(s, StatusCode::CREATED, "body: {created}");
    let id = created["id"].as_str().unwrap().to_string();
    let (_, result) = send(&st, "GET", &format!("/backtests/{id}/result"), None).await;
    assert!(result["trades"].as_array().unwrap().iter().any(|t| t["status"] == "executed"));
}
