#![allow(dead_code)]

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    status: u16,
    detail: String,
}

#[derive(Debug)]
pub(crate) enum ApiError {
    Unauthorized(&'static str),
    Forbidden(&'static str),
    BadRequest(String),
    TooManyRequests(&'static str),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Unauthorized(message) => {
                let status = StatusCode::UNAUTHORIZED;
                let mut response = (
                    status,
                    Json(ErrorResponse { status: status.as_u16(), detail: message.to_string() }),
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
                    Json(ErrorResponse { status: status.as_u16(), detail: message.to_string() }),
                )
                    .into_response()
            }
            ApiError::BadRequest(message) => {
                let status = StatusCode::BAD_REQUEST;
                (status, Json(ErrorResponse { status: status.as_u16(), detail: message }))
                    .into_response()
            }
            ApiError::TooManyRequests(message) => {
                let status = StatusCode::TOO_MANY_REQUESTS;
                (
                    status,
                    Json(ErrorResponse { status: status.as_u16(), detail: message.to_string() }),
                )
                    .into_response()
            }
            ApiError::Internal(message) => {
                let status = StatusCode::INTERNAL_SERVER_ERROR;
                (status, Json(ErrorResponse { status: status.as_u16(), detail: message }))
                    .into_response()
            }
        }
    }
}
