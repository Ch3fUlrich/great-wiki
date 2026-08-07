use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    /// The server is misconfigured in a way that makes it unsafe to answer — currently
    /// only an enforced proxy boundary with no secret behind it.
    #[error("service unavailable")]
    Unavailable,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found"),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            // No detail: which piece of configuration is missing is not a client's business.
            ApiError::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "service unavailable"),
            ApiError::Internal(e) => {
                // Log the detail, return none of it: internal errors carry filesystem
                // paths and SQL, which must not reach a client.
                tracing::error!(error = ?e, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            }
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
