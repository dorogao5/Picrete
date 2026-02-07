use std::sync::{Mutex, MutexGuard, OnceLock};

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request},
    Router,
};
use sqlx::PgPool;
use time::{OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

use crate::api;
use crate::core::{config::Settings, redis::RedisHandle, security, state::AppState};
use crate::db::models::User;
use crate::db::types::UserRole;
use crate::services::storage::StorageService;

const TEST_DATABASE_URL: &str =
    "postgresql://picrete_test:picrete_test@localhost:5432/picrete_rust_test";
const TEST_SECRET_KEY: &str = "test-secret";
const TEST_REDIS_DB: &str = "1";

pub(crate) struct TestContext {
    pub(crate) state: AppState,
    pub(crate) app: Router,
    _guard: MutexGuard<'static, ()>,
}

pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|err| err.into_inner())
}

pub(crate) fn set_test_env() {
    // Load .env so REDIS_PASSWORD and other settings are available
    dotenvy::dotenv().ok();

    std::env::set_var("PICRETE_ENV", "test");
    std::env::set_var("PICRETE_STRICT_CONFIG", "0");
    std::env::set_var("SECRET_KEY", TEST_SECRET_KEY);
    std::env::set_var("DATABASE_URL", TEST_DATABASE_URL);
    std::env::set_var("REDIS_HOST", "127.0.0.1");
    std::env::set_var("REDIS_PORT", "6379");
    std::env::set_var("REDIS_DB", TEST_REDIS_DB);
    // Keep REDIS_PASSWORD from .env if set (don't remove it)
    std::env::set_var("PROMETHEUS_ENABLED", "0");
    std::env::remove_var("S3_ENDPOINT");
    std::env::remove_var("S3_ACCESS_KEY");
    std::env::remove_var("S3_SECRET_KEY");
    std::env::remove_var("S3_BUCKET");
    std::env::remove_var("S3_REGION");
    std::env::set_var("AWS_EC2_METADATA_DISABLED", "true");
}

pub(crate) fn set_test_storage_env() {
    std::env::set_var("S3_ENDPOINT", "http://localhost:9000");
    std::env::set_var("S3_ACCESS_KEY", "test-access-key");
    std::env::set_var("S3_SECRET_KEY", "test-secret-key");
    std::env::set_var("S3_BUCKET", "picrete-test-bucket");
    std::env::set_var("S3_REGION", "ru-central1");
}

pub(crate) async fn setup_test_context() -> TestContext {
    let guard = env_lock();
    set_test_env();

    let settings = Settings::load().expect("settings");
    let db = prepare_db(&settings).await;

    let redis = RedisHandle::new(settings.redis().redis_url());
    redis.connect().await.expect("redis connect");
    reset_redis(settings.redis().redis_url()).await.expect("redis reset");

    let state = AppState::new(settings, db, redis, None);
    let app = api::router::router(state.clone());

    TestContext { state, app, _guard: guard }
}

pub(crate) async fn setup_test_context_with_storage() -> TestContext {
    let guard = env_lock();
    set_test_env();
    set_test_storage_env();

    let settings = Settings::load().expect("settings");
    let db = prepare_db(&settings).await;

    let redis = RedisHandle::new(settings.redis().redis_url());
    redis.connect().await.expect("redis connect");
    reset_redis(settings.redis().redis_url()).await.expect("redis reset");

    let storage = StorageService::from_settings(&settings).await.expect("storage service");

    let state = AppState::new(settings, db, redis, storage);
    let app = api::router::router(state.clone());

    TestContext { state, app, _guard: guard }
}

async fn prepare_db(settings: &Settings) -> PgPool {
    let db = crate::db::init_pool(settings).await.expect("db pool");
    let current_db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&db)
        .await
        .expect("current database");
    assert_eq!(current_db, "picrete_rust_test");

    ensure_schema(&db).await.expect("schema");
    let has_id: Option<i32> = sqlx::query_scalar(
        "SELECT 1 FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = 'users' AND column_name = 'id'",
    )
    .fetch_optional(&db)
    .await
    .expect("users schema");
    assert!(has_id.is_some(), "users.id missing");

    reset_db(&db).await.expect("reset db");
    db
}

pub(crate) async fn ensure_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations").run(pool).await.map_err(|e| sqlx::Error::Configuration(e.into()))?;
    Ok(())
}

pub(crate) async fn reset_db(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "TRUNCATE submission_scores, submission_images, submissions, exam_sessions, \
         task_variants, task_types, exams, users RESTART IDENTITY CASCADE",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn reset_redis(url: String) -> redis::RedisResult<()> {
    let client = redis::Client::open(url)?;
    let mut manager = redis::aio::ConnectionManager::new(client).await?;
    redis::cmd("FLUSHDB").query_async::<_, ()>(&mut manager).await?;
    Ok(())
}

pub(crate) async fn insert_user(
    pool: &PgPool,
    isu: &str,
    full_name: &str,
    role: UserRole,
    password: &str,
) -> User {
    let hashed_password = security::hash_password(password).expect("hash password");
    let now_offset = OffsetDateTime::now_utc();
    let now = PrimitiveDateTime::new(now_offset.date(), now_offset.time());

    sqlx::query_as::<_, User>(
        "INSERT INTO users (
            id, isu, hashed_password, full_name, role, is_active, is_verified,
            pd_consent, created_at, updated_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
         RETURNING id, isu, hashed_password, full_name, role, is_active, is_verified,
            pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at,
            terms_version, privacy_version, created_at, updated_at",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(isu)
    .bind(hashed_password)
    .bind(full_name)
    .bind(role)
    .bind(true)
    .bind(true)
    .bind(false)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await
    .expect("insert user")
}

pub(crate) fn bearer_token(user_id: &str, settings: &Settings) -> String {
    security::create_access_token(user_id, settings, None).expect("token")
}

pub(crate) fn json_request(
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);

    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    if let Some(body) = body {
        let bytes = serde_json::to_vec(&body).expect("serialize body");
        builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(bytes))
            .expect("request body")
    } else {
        builder.body(Body::empty()).expect("request body")
    }
}

pub(crate) async fn read_json(response: axum::response::Response<Body>) -> serde_json::Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.expect("response body");
    serde_json::from_slice(&body).unwrap_or_else(|err| {
        let body_text = String::from_utf8_lossy(&body);
        panic!("json parse: {err}; body: {body_text}");
    })
}

