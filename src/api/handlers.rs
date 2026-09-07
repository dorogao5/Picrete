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
            tracing::warn!(error = %error, dependency = "redis", "Health dependency check failed");
            components.insert("redis".to_string(), "unhealthy".to_string());
            status = "degraded".to_string();
        }
    }

    match crate::repositories::health::ping(state.db()).await {
        Ok(_) => {
            components.insert("database".to_string(), "healthy".to_string());
        }
        Err(err) => {
            tracing::warn!(error = %err, dependency = "database", "Health dependency check failed");
            components.insert("database".to_string(), "unhealthy".to_string());
            status = "unhealthy".to_string();
        }
    }

    Json(HealthResponse { service: "picrete-api".to_string(), status, components })
}

pub(crate) async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let mut components = HashMap::new();

    match crate::repositories::health::ping(state.db()).await {
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
            tracing::warn!(error = %err, dependency = "database", "Readiness dependency check failed");
            components.insert("database".to_string(), "not_ready".to_string());
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

pub(crate) async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "picrete-api",
        "revision": option_env!("BUILD_REVISION").unwrap_or("development"),
        "studio_snapshot_schema": 1,
    }))
}
