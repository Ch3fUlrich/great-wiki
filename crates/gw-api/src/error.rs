use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    /// The request was understood and refused on its own terms — a password below the
    /// length floor, a missing field. The message is written here, never derived from an
    /// internal error, so nothing about the database can reach a client through it.
    #[error("{0}")]
    Invalid(String),
    /// The request conflicts with what is already there: a username or a team slug that
    /// is taken. Distinguished from [`ApiError::Invalid`] because the fix is different —
    /// pick another name rather than correct this one.
    #[error("{0}")]
    Conflict(String),
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
            ApiError::Invalid(message) => (StatusCode::BAD_REQUEST, message.as_str()),
            ApiError::Conflict(message) => (StatusCode::CONFLICT, message.as_str()),
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
