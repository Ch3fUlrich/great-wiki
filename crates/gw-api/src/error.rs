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
    /// More than the endpoint will accept — today only an upload past
    /// [`gw_store::MAX_ATTACHMENT_BYTES`] (D-17). Distinguished from
    /// [`ApiError::Invalid`] because the request was well formed and the fix is a smaller
    /// file, not a corrected one.
    #[error("{0}")]
    TooLarge(String),
    /// The bytes are not something this wiki will store. Distinguished from
    /// [`ApiError::Invalid`] for the same reason: nothing about the request is malformed,
    /// and no amount of correcting it will help — see `gw_store::blobs::sniff` for the
    /// allowlist and why it is closed.
    #[error("{0}")]
    Unsupported(String),
    /// The server is misconfigured in a way that makes it unsafe to answer, **or** it knows
    /// about something it cannot reach right now.
    ///
    /// Two cases today: an enforced proxy boundary with no secret behind it, and an
    /// attachment whose row is in the database while its bytes are not on the mount. The
    /// second is deliberately NOT a 404 — the wiki knows the file exists and is failing to
    /// serve it, which is a different statement and sends whoever is looking into it at the
    /// mount rather than at the database. `/mnt/cloud` really does answer `Stale file
    /// handle` inside a container while the host is fine, and it recovers, which is exactly
    /// what 503 means.
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
            ApiError::TooLarge(message) => (StatusCode::PAYLOAD_TOO_LARGE, message.as_str()),
            ApiError::Unsupported(message) => {
                (StatusCode::UNSUPPORTED_MEDIA_TYPE, message.as_str())
            }
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
