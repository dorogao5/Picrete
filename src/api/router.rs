use axum::{
    http::header::{HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, ORIGIN},
    http::{HeaderName, Method, Request, Response},
    routing::get,
    Router,
};
use std::time::Duration;
use tower_http::{
    cors::{AllowOrigin, Any, CorsLayer},
    normalize_path::NormalizePathLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::Span;

use crate::api::auth;
use crate::api::exams;
use crate::api::handlers;
use crate::api::submissions;
use crate::api::users;
use crate::core::{config::Settings, state::AppState};

pub(crate) fn router(state: AppState) -> Router {
    let cors = build_cors_layer(state.settings());
    let api_v1_prefix = state.settings().api().api_v1_str.clone();
    let api_v1 = Router::new()
        .nest("/auth", auth::router())
        .nest("/users", users::router())
        .nest("/exams", exams::router())
        .nest("/submissions", submissions::router());

    let request_id_header = HeaderName::from_static("x-request-id");
    let request_id_header_for_span = request_id_header.clone();
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(move |request: &Request<_>| {
            let request_id = request
                .headers()
                .get(&request_id_header_for_span)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("-");
            tracing::info_span!(
                "request",
                method = %request.method(),
                uri = %request.uri(),
                request_id = %request_id
            )
        })
        .on_response(|response: &Response<axum::body::Body>, latency: Duration, _span: &Span| {
            let status_label = response.status().as_u16().to_string();
            metrics::counter!(
                "http_requests_total",
                "status" => status_label.clone()
            )
            .increment(1);
            metrics::histogram!(
                "http_request_duration_seconds",
                "status" => status_label
            )
            .record(latency.as_secs_f64());
        });

    let mut router: Router<AppState> = Router::new()
        .route("/", get(handlers::root))
        .route("/healthz", get(handlers::healthz).head(handlers::healthz))
        .nest(&api_v1_prefix, api_v1)
        .layer(NormalizePathLayer::trim_trailing_slash())
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(trace_layer)
        .layer(cors);

    if state.settings().telemetry().prometheus_enabled {
        router = router.route("/metrics", get(handlers::metrics));
    }

    router.with_state(state)
}

fn build_cors_layer(settings: &Settings) -> CorsLayer {
    let origins = settings
        .cors()
        .origins
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect::<Vec<_>>();

    let base = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            ACCEPT,
            ORIGIN,
            HeaderName::from_static("x-request-id"),
        ])
        .expose_headers([HeaderName::from_static("x-request-id")])
        .max_age(Duration::from_secs(3600));

    if origins.is_empty() {
        // Wildcard origin cannot be combined with allow_credentials
        base.allow_origin(Any)
    } else {
        base.allow_credentials(true)
            .allow_origin(AllowOrigin::list(origins))
    }
}

#[cfg(test)]
mod tests {
    use super::router;
    use axum::{body::to_bytes, body::Body, http::Request, http::StatusCode};
    use tower::ServiceExt;

    use crate::core::redis::RedisHandle;
    use crate::core::state::AppState;
    use crate::core::{config::Settings, metrics};
    use crate::test_support;

    fn build_state(settings: Settings) -> AppState {
        let db =
            sqlx::PgPool::connect_lazy(&settings.database().database_url()).expect("lazy pool");
        let redis = RedisHandle::new(settings.redis().redis_url());
        AppState::new(settings, db, redis, None)
    }

    #[tokio::test]
    async fn root_returns_message() {
        let _guard = test_support::env_lock();
        std::env::set_var("SECRET_KEY", "test-secret");
        std::env::remove_var("PROMETHEUS_ENABLED");

        let settings = Settings::load().expect("settings");
        let app = router(build_state(settings));

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["message"], "Picrete Rust API");
    }

    #[tokio::test]
    async fn metrics_disabled_returns_404() {
        let _guard = test_support::env_lock();
        std::env::set_var("SECRET_KEY", "test-secret");
        std::env::remove_var("PROMETHEUS_ENABLED");

        let settings = Settings::load().expect("settings");
        let app = router(build_state(settings));

        let response = app
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn metrics_enabled_returns_200() {
        let _guard = test_support::env_lock();
        std::env::set_var("SECRET_KEY", "test-secret");
        std::env::set_var("PROMETHEUS_ENABLED", "1");

        let settings = Settings::load().expect("settings");
        metrics::init(&settings).expect("metrics init");
        let app = router(build_state(settings));

        let response = app
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }
}
