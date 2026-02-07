use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use std::collections::HashMap;

use crate::core::metrics;
use crate::core::state::AppState;
use crate::schemas::{HealthResponse, RootResponse};

pub(crate) async fn root(State(state): State<AppState>) -> Json<RootResponse> {
    let api = state.settings().api();
    let response = RootResponse {
        message: api.project_name.clone(),
        version: api.version.clone(),
        docs_url: format!("{}/docs", api.api_v1_str),
    };

    Json(response)
}

pub(crate) async fn healthz(State(state): State<AppState>) -> Json<HealthResponse> {
    let mut status = "healthy".to_string();
    let mut components = HashMap::new();

    match state.redis().health().await {
        crate::core::redis::RedisHealth::Healthy => {
            components.insert("redis".to_string(), "healthy".to_string());
        }
        crate::core::redis::RedisHealth::Disconnected => {
            components.insert("redis".to_string(), "disconnected".to_string());
        }
        crate::core::redis::RedisHealth::Unhealthy(error) => {
            components.insert("redis".to_string(), format!("unhealthy: {error}"));
            status = "degraded".to_string();
        }
    }

    match sqlx::query("SELECT 1").execute(state.db()).await {
        Ok(_) => {
            components.insert("database".to_string(), "healthy".to_string());
        }
        Err(err) => {
            components.insert("database".to_string(), format!("unhealthy: {err}"));
            status = "unhealthy".to_string();
        }
    }

    Json(HealthResponse { service: "picrete-api".to_string(), status, components })
}

pub(crate) async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let mut components = HashMap::new();

    match sqlx::query("SELECT 1").execute(state.db()).await {
        Ok(_) => {
            components.insert("database".to_string(), "ready".to_string());
            (
                StatusCode::OK,
                Json(HealthResponse {
                    service: "picrete-api".to_string(),
                    status: "ready".to_string(),
                    components,
                }),
            )
                .into_response()
        }
        Err(err) => {
            components.insert("database".to_string(), format!("not_ready: {err}"));
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    service: "picrete-api".to_string(),
                    status: "not_ready".to_string(),
                    components,
                }),
            )
                .into_response()
        }
    }
}

pub(crate) async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    if !state.settings().telemetry().prometheus_enabled {
        return StatusCode::NOT_FOUND.into_response();
    }

    match metrics::render() {
        Some(body) => ([(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")], body)
            .into_response(),
        None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
