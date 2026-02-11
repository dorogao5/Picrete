use std::sync::{Arc, OnceLock};

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request},
    Router,
};
use sqlx::PgPool;
use tokio::sync::{Mutex, OwnedMutexGuard};
use uuid::Uuid;

use crate::api;
use crate::core::{
    config::Settings, redis::RedisHandle, security, state::AppState, time::primitive_now_utc,
};
use crate::db::models::{Course, User};
use crate::db::types::CourseRole;
use crate::repositories;
use crate::services::invite_codes;
use crate::services::storage::StorageService;

const TEST_DATABASE_URL: &str =
    "postgresql://picrete_test:picrete_test@localhost:5432/picrete_rust_test";
const TEST_SECRET_KEY: &str = "test-secret";
const TEST_REDIS_DB: &str = "1";

pub(crate) struct TestContext {
    pub(crate) state: AppState,
    pub(crate) app: Router,
    _guard: OwnedMutexGuard<()>,
}

pub(crate) async fn env_lock() -> OwnedMutexGuard<()> {
    static LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Arc::new(Mutex::new(()))).clone();
    lock.lock_owned().await
}

pub(crate) fn set_test_env() {
    // Load .env so REDIS_PASSWORD and other settings are available
    dotenvy::dotenv().ok();

    std::env::set_var("PICRETE_ENV", "test");
    std::env::set_var("PICRETE_STRICT_CONFIG", "0");
    std::env::set_var("COURSE_CONTEXT_MODE", "route");
    std::env::set_var("SECRET_KEY", TEST_SECRET_KEY);
    std::env::set_var("DATABASE_URL", TEST_DATABASE_URL);
    std::env::set_var("REDIS_HOST", "127.0.0.1");
    std::env::set_var("REDIS_PORT", "6379");
    std::env::set_var("REDIS_DB", TEST_REDIS_DB);
    std::env::remove_var("REDIS_PASSWORD");
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
    let guard = env_lock().await;
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
    let guard = env_lock().await;
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

    reset_public_schema(&db).await.expect("reset schema");
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

async fn reset_public_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DROP SCHEMA IF EXISTS public CASCADE").execute(pool).await?;
    sqlx::query("CREATE SCHEMA public").execute(pool).await?;
    Ok(())
}

pub(crate) async fn ensure_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    let migrations_dir =
        std::env::var("PICRETE_MIGRATIONS_DIR").unwrap_or_else(|_| "migrations".to_string());
    let mut migrator = sqlx::migrate::Migrator::new(std::path::Path::new(&migrations_dir))
        .await
        .map_err(|error| sqlx::Error::Migrate(Box::new(error)))?;
    migrator.set_ignore_missing(true);
    migrator.run(pool).await.map_err(|error| sqlx::Error::Migrate(Box::new(error)))?;
    Ok(())
}

pub(crate) async fn reset_db(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "TRUNCATE trainer_set_items, trainer_sets, task_bank_item_images, task_bank_items, \
         task_bank_sources, submission_scores, submission_images, telegram_selected_sessions, \
         telegram_user_links, telegram_bot_offsets, submissions, exam_sessions, \
         task_variants, task_types, exams, course_membership_roles, course_invite_codes, \
         course_memberships, course_identity_policies, courses, users RESTART IDENTITY CASCADE",
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
    username: &str,
    full_name: &str,
    password: &str,
) -> User {
    insert_user_with_admin(pool, username, full_name, password, false).await
}

pub(crate) async fn insert_platform_admin(
    pool: &PgPool,
    username: &str,
    full_name: &str,
    password: &str,
) -> User {
    insert_user_with_admin(pool, username, full_name, password, true).await
}

pub(crate) async fn insert_user_with_admin(
    pool: &PgPool,
    username: &str,
    full_name: &str,
    password: &str,
    is_platform_admin: bool,
) -> User {
    let hashed_password = security::hash_password(password).expect("hash password");
    let now = primitive_now_utc();

    repositories::users::create(
        pool,
        repositories::users::CreateUser {
            id: &Uuid::new_v4().to_string(),
            username,
            hashed_password,
            full_name,
            is_platform_admin,
            is_active: true,
            pd_consent: false,
            pd_consent_at: None,
            pd_consent_version: None,
            terms_accepted_at: None,
            terms_version: None,
            privacy_version: None,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .expect("insert user")
}

pub(crate) async fn insert_course(
    pool: &PgPool,
    slug: &str,
    title: &str,
    created_by: &str,
) -> Course {
    let now = primitive_now_utc();
    let course = repositories::courses::create(
        pool,
        repositories::courses::CreateCourse {
            id: &Uuid::new_v4().to_string(),
            slug,
            title,
            organization: None,
            is_active: true,
            created_by,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .expect("insert course");

    repositories::courses::ensure_default_identity_policy(pool, &course.id, now)
        .await
        .expect("ensure identity policy");

    course
}

pub(crate) async fn create_course_with_teacher(
    pool: &PgPool,
    slug: &str,
    title: &str,
    teacher_id: &str,
) -> Course {
    let course = insert_course(pool, slug, title, teacher_id).await;
    add_course_role(pool, &course.id, teacher_id, CourseRole::Teacher).await;
    course
}

pub(crate) async fn add_course_role(
    pool: &PgPool,
    course_id: &str,
    user_id: &str,
    role: CourseRole,
) -> String {
    repositories::course_memberships::ensure_membership_with_role(
        pool,
        repositories::course_memberships::EnsureMembershipParams {
            course_id,
            user_id,
            invited_by: None,
            identity_payload: serde_json::json!({}),
            role,
            joined_at: primitive_now_utc(),
        },
    )
    .await
    .expect("add course role")
}

pub(crate) async fn create_active_invite_code(
    pool: &PgPool,
    course: &Course,
    role: CourseRole,
) -> String {
    let now = primitive_now_utc();

    if let Some(existing) =
        repositories::course_invites::find_active_for_course_role(pool, &course.id, role)
            .await
            .expect("find active invite")
    {
        repositories::course_invites::deactivate(pool, &existing.id, now)
            .await
            .expect("deactivate previous invite");
    }

    let code = invite_codes::generate_invite_code(&course.slug, role);
    let code_hash = invite_codes::hash_invite_code(&code);

    repositories::course_invites::create(
        pool,
        repositories::course_invites::CreateInviteCode {
            id: &Uuid::new_v4().to_string(),
            course_id: &course.id,
            role,
            code_hash: &code_hash,
            is_active: true,
            rotated_from_id: None,
            expires_at: None,
            usage_count: 0,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .expect("create invite code");

    code
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
