use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// Errors a request can end in; each maps to an HTTP status with a JSON `{"error": ...}`.
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    Unauthorized,
    Forbidden(String),
    Sqlite(rusqlite::Error),
    Internal(anyhow::Error),
}

// `?` on a rusqlite or anyhow error inside a handler converts it through these impls.
impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        AppError::Sqlite(error)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        AppError::Internal(error)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            AppError::NotFound(what) => (StatusCode::NOT_FOUND, format!("{what} not found")),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "missing or unknown token".into()),
            AppError::Forbidden(message) => (StatusCode::FORBIDDEN, message),
            AppError::Sqlite(error) => {
                tracing::error!("database: {error}");
                (StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
            }
            AppError::Internal(error) => {
                tracing::error!("internal: {error:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
            }
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
