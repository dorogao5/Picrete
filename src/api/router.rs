use axum::{
    body::Body,
    extract::DefaultBodyLimit,
    http::header::{
        HeaderValue, ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, ORIGIN,
        STRICT_TRANSPORT_SECURITY,
    },
    http::{HeaderName, Method, Request, Response},
    middleware::{self, Next},
    routing::get,
    Router,
};
use std::time::Duration;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    normalize_path::NormalizePathLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::Span;

use crate::api::assistant;
use crate::api::auth;
use crate::api::courses;
use crate::api::exams;
use crate::api::handlers;
use crate::api::materials;
use crate::api::submissions;
use crate::api::task_bank;
use crate::api::trainer;
use crate::api::users;
use crate::core::{config::Settings, state::AppState};

pub(crate) fn router(state: AppState) -> Router {
    let course_context_mode = state.settings().course().context_mode.as_str();
    if course_context_mode != "route" {
        panic!(
            "Unsupported COURSE_CONTEXT_MODE='{course_context_mode}'. Only 'route' is supported in this release."
        );
    }

    let cors = build_cors_layer(state.settings());
    let api_v1_prefix = state.settings().api().api_v1_str.clone();
    // Axum по умолчанию ограничивает тело запроса 2MB — меньше, чем MAX_UPLOAD_SIZE_MB,
    // из-за чего загрузка фото падала до проверки лимита. +1MB на multipart-обвязку.
    let body_limit_bytes =
        (state.settings().storage().max_upload_size_mb as usize + 1) * 1024 * 1024;
    let api_v1 = Router::new()
        .nest("/auth", auth::router())
        .nest("/users", users::router())
        .nest("/courses", courses::router())
        .nest("/courses/:course_id/exams", exams::router())
        .nest("/courses/:course_id/submissions", submissions::router())
        .nest("/courses/:course_id/task-bank", task_bank::router())
        .nest("/courses/:course_id/trainer", trainer::router())
        .nest("/courses/:course_id/materials", materials::router())
        .nest("/courses/:course_id/assistant", assistant::router())
        .nest("/internal/studio", assistant::internal_router())
        .layer(DefaultBodyLimit::max(body_limit_bytes));

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

    let production = state.settings().runtime().environment.as_str() == "production";
    let mut router: Router<AppState> = Router::new()
        .route("/", get(handlers::root))
        .route("/version", get(handlers::version))
        .route("/healthz", get(handlers::healthz).head(handlers::healthz))
        .route("/readyz", get(handlers::readyz).head(handlers::readyz))
        .nest(&api_v1_prefix, api_v1)
        .layer(NormalizePathLayer::trim_trailing_slash())
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(trace_layer)
        .layer(middleware::from_fn(add_security_headers))
        .layer(cors);

    if production {
        router = router.layer(middleware::from_fn(add_hsts));
    }

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
        .map(|origin| {
            HeaderValue::from_str(origin)
                .expect("CORS origins are validated while application settings are loaded")
        })
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

    base.allow_credentials(true).allow_origin(AllowOrigin::list(origins))
}

async fn add_security_headers(request: Request<Body>, next: Next) -> Response<Body> {
    let protected_api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(HeaderName::from_static("x-frame-options"), HeaderValue::from_static("DENY"));
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-site"),
    );
    if protected_api {
        headers.insert(
            CACHE_CONTROL,
            HeaderValue::from_static("private, no-store, max-age=0, must-revalidate"),
        );
    }
    response
}

async fn add_hsts(request: Request<Body>, next: Next) -> Response<Body> {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::router;
    use axum::{body::to_bytes, body::Body, http::Request, http::StatusCode};
    use sqlx::postgres::PgPoolOptions;
    use std::time::Duration;
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
        let _guard = test_support::env_lock().await;
        std::env::set_var("SECRET_KEY", "test-secret");
        std::env::set_var("COURSE_CONTEXT_MODE", "route");
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
        assert_eq!(json["message"], "Picrete API");
    }

    #[tokio::test]
    async fn metrics_disabled_returns_404() {
        let _guard = test_support::env_lock().await;
        std::env::set_var("SECRET_KEY", "test-secret");
        std::env::set_var("COURSE_CONTEXT_MODE", "route");
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
        let _guard = test_support::env_lock().await;
        std::env::set_var("SECRET_KEY", "test-secret");
        std::env::set_var("COURSE_CONTEXT_MODE", "route");
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

    #[tokio::test]
    async fn health_and_readiness_succeed_without_sensitive_details() {
        let ctx = test_support::setup_test_context().await;

        let health = ctx
            .app
            .clone()
            .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
            .await
            .expect("health response");
        assert_eq!(health.status(), StatusCode::OK);
        let health_body = to_bytes(health.into_body(), usize::MAX).await.unwrap();
        let health_json: serde_json::Value = serde_json::from_slice(&health_body).unwrap();
        assert_eq!(health_json["components"]["database"], "healthy");
        assert_eq!(health_json["components"]["redis"], "healthy");

        let ready = ctx
            .app
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
            .await
            .expect("readiness response");
        assert_eq!(ready.status(), StatusCode::OK);
        let ready_body = to_bytes(ready.into_body(), usize::MAX).await.unwrap();
        let ready_json: serde_json::Value = serde_json::from_slice(&ready_body).unwrap();
        assert_eq!(ready_json["status"], "ready");
        assert_eq!(ready_json["components"]["database"], "ready");
    }

    #[tokio::test]
    async fn degraded_readiness_is_opaque() {
        let _guard = test_support::env_lock().await;
        test_support::set_test_env();
        let settings = Settings::load().expect("settings");
        let unavailable_url = "postgresql://audit_user:audit_password@127.0.0.1:1/audit_db";
        let db = PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(250))
            .connect_lazy(unavailable_url)
            .expect("lazy unavailable pool");
        let redis = RedisHandle::new(settings.redis().redis_url());
        let app = router(AppState::new(settings, db, redis, None));

        let response = app
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
            .await
            .expect("readiness response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "not_ready");
        assert_eq!(json["components"]["database"], "not_ready");

        let text = String::from_utf8(body.to_vec()).unwrap();
        for secret in ["audit_user", "audit_password", "127.0.0.1", "connection refused"] {
            assert!(
                !text.to_ascii_lowercase().contains(secret),
                "leaked dependency detail: {text}"
            );
        }
    }
}
