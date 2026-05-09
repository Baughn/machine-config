use std::io;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Daemon-handler error type. Mapped to plain text/plain 4xx/5xx responses.
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Internal(String),
}

impl From<io::Error> for AppError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => {
                AppError::BadRequest(err.to_string())
            }
            _ => AppError::Internal(err.to_string()),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, [("content-type", "text/plain")], body).into_response()
    }
}
