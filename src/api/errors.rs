use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    status: u16,
    code: &'static str,
    detail: String,
}

#[derive(Debug)]
pub(crate) enum ApiError {
    Unauthorized(&'static str),
    Forbidden(&'static str),
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    TooManyRequests(&'static str),
    ServiceUnavailable(String),
    UnprocessableEntity(String),
    Internal(String),
}

impl ApiError {
    /// Log the underlying error with context and return an `Internal` variant.
    pub(crate) fn internal(err: impl std::fmt::Display, context: &str) -> Self {
        tracing::error!(error = %err, "{context}");
        Self::Internal(context.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Unauthorized(message) => {
                let status = StatusCode::UNAUTHORIZED;
                let mut response = (
                    status,
                    Json(ErrorResponse {
                        status: status.as_u16(),
                        code: "AUTH_UNAUTHORIZED",
                        detail: message.to_string(),
                    }),
                )
                    .into_response();
                response
                    .headers_mut()
                    .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
                response
            }
            ApiError::Forbidden(message) => {
                let status = StatusCode::FORBIDDEN;
                (
                    status,
                    Json(ErrorResponse {
                        status: status.as_u16(),
                        code: "COURSE_ACCESS_DENIED",
                        detail: message.to_string(),
                    }),
                )
                    .into_response()
            }
            ApiError::BadRequest(message) => {
                let status = StatusCode::BAD_REQUEST;
                (
                    status,
                    Json(ErrorResponse {
                        status: status.as_u16(),
                        code: "BAD_REQUEST",
                        detail: message,
                    }),
                )
                    .into_response()
            }
            ApiError::NotFound(message) => {
                let status = StatusCode::NOT_FOUND;
                (
                    status,
                    Json(ErrorResponse {
                        status: status.as_u16(),
                        code: "NOT_FOUND",
                        detail: message,
                    }),
                )
                    .into_response()
            }
            ApiError::Conflict(message) => {
                let status = StatusCode::CONFLICT;
                (
                    status,
                    Json(ErrorResponse {
                        status: status.as_u16(),
                        code: "CONFLICT",
                        detail: message,
                    }),
                )
                    .into_response()
            }
            ApiError::TooManyRequests(message) => {
                let status = StatusCode::TOO_MANY_REQUESTS;
                (
                    status,
                    Json(ErrorResponse {
                        status: status.as_u16(),
                        code: "RATE_LIMITED",
                        detail: message.to_string(),
                    }),
                )
                    .into_response()
            }
            ApiError::ServiceUnavailable(message) => {
                let status = StatusCode::SERVICE_UNAVAILABLE;
                (
                    status,
                    Json(ErrorResponse {
                        status: status.as_u16(),
                        code: "SERVICE_UNAVAILABLE",
                        detail: message,
                    }),
                )
                    .into_response()
            }
            ApiError::UnprocessableEntity(message) => {
                let status = StatusCode::UNPROCESSABLE_ENTITY;
                (
                    status,
                    Json(ErrorResponse {
                        status: status.as_u16(),
                        code: "VALIDATION_FAILED",
                        detail: message,
                    }),
                )
                    .into_response()
            }
            ApiError::Internal(message) => {
                let status = StatusCode::INTERNAL_SERVER_ERROR;
                (
                    status,
                    Json(ErrorResponse {
                        status: status.as_u16(),
                        code: "INTERNAL_ERROR",
                        detail: message,
                    }),
                )
                    .into_response()
            }
        }
    }
}
