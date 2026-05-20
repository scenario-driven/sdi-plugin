//! Map `DomainError` to a JSON HTTP response.
//!
//! Body shape (stable contract — CLI/web parse this):
//! ```json
//! { "error": { "code": "EVIDENCE_REQUIRED", "message": "...", "status": 400 } }
//! ```

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use sdi_core::error::DomainError;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError(pub DomainError);

pub type ApiResult<T> = Result<T, ApiError>;

impl From<DomainError> for ApiError {
    fn from(e: DomainError) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = self.0.code();
        let status = self.0.http_status();
        let body = json!({
            "error": {
                "code": code,
                "message": self.0.to_string(),
                "status": status,
            }
        });
        (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
            Json(body),
        )
            .into_response()
    }
}
