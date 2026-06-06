//! Structured error responses.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: ErrorDetail,
    #[serde(flatten)]
    extra: Value,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

/// A structured `{ "error": { code, message }, ...extra }` response.
pub fn error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    error_with(status, code, message, Value::Object(Default::default()))
}

pub fn error_with(status: StatusCode, code: &str, message: impl Into<String>, extra: Value) -> Response {
    (
        status,
        Json(ErrorBody {
            error: ErrorDetail { code: code.into(), message: message.into() },
            extra,
        }),
    )
        .into_response()
}
