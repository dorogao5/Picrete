use axum::{
    extract::{Form, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use time::{OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::CurrentUser;
use crate::core::security;
use crate::core::state::AppState;
use crate::db::models::User;
use crate::db::types::UserRole;
use crate::schemas::auth::TokenResponse;
use crate::schemas::user::{UserCreate, UserLogin, UserResponse};

/// Max attempts per window for auth endpoints (login/signup/token).
const AUTH_RATE_LIMIT: u64 = 10;
/// Rate limit window in seconds.
const AUTH_RATE_WINDOW_SECONDS: u64 = 60;

#[derive(Debug, Deserialize)]
struct OAuth2PasswordForm {
    username: String,
    password: String,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/signup", post(signup))
        .route("/login", post(login))
        .route("/token", post(token))
        .route("/me", get(me))
}

async fn signup(
    State(state): State<AppState>,
    Json(payload): Json<UserCreate>,
) -> Result<(StatusCode, Json<TokenResponse>), ApiError> {
    validate_isu(&payload.isu)?;

    let rate_key = format!("rl:signup:{}", payload.isu);
    let allowed = state.redis().rate_limit(&rate_key, AUTH_RATE_LIMIT, AUTH_RATE_WINDOW_SECONDS)
        .await
        .unwrap_or(true);
    if !allowed {
        return Err(ApiError::TooManyRequests("Too many signup attempts, try again later"));
    }

    if !payload.pd_consent {
        return Err(ApiError::BadRequest("Personal data consent is required".to_string()));
    }

    let existing = sqlx::query_scalar::<_, String>("SELECT id FROM users WHERE isu = $1")
        .bind(&payload.isu)
        .fetch_optional(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to check existing user".to_string()))?;

    if existing.is_some() {
        return Err(ApiError::BadRequest("User with this ISU already exists".to_string()));
    }

    let hashed_password = security::hash_password(&payload.password)
        .map_err(|_| ApiError::Internal("Failed to hash password".to_string()))?;

    let now_offset = OffsetDateTime::now_utc();
    let now_primitive = primitive_now_utc(now_offset);

    let pd_consent_version = payload
        .pd_consent_version
        .clone()
        .unwrap_or_else(|| state.settings().api().pd_consent_version.clone());
    let terms_version = payload
        .terms_version
        .clone()
        .unwrap_or_else(|| state.settings().api().terms_version.clone());
    let privacy_version = payload
        .privacy_version
        .clone()
        .unwrap_or_else(|| state.settings().api().privacy_version.clone());

    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (
            id, isu, hashed_password, full_name, role, is_active, is_verified,
            pd_consent, pd_consent_at, pd_consent_version,
            terms_accepted_at, terms_version, privacy_version,
            created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
        RETURNING id, isu, hashed_password, full_name, role, is_active, is_verified,
            pd_consent, pd_consent_at, pd_consent_version,
            terms_accepted_at, terms_version, privacy_version,
            created_at, updated_at",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&payload.isu)
    .bind(hashed_password)
    .bind(&payload.full_name)
    .bind(UserRole::Student)
    .bind(true)
    .bind(false)
    .bind(true)
    .bind(Some(now_offset))
    .bind(pd_consent_version)
    .bind(Some(now_offset))
    .bind(terms_version)
    .bind(privacy_version)
    .bind(now_primitive)
    .bind(now_primitive)
    .fetch_one(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to create user".to_string()))?;

    let token = security::create_access_token(&user.id, state.settings(), None)
        .map_err(|_| ApiError::Internal("Failed to create access token".to_string()))?;

    let response = TokenResponse {
        access_token: token,
        token_type: "bearer".to_string(),
        user: UserResponse::from_db(user),
    };

    Ok((StatusCode::CREATED, Json(response)))
}

async fn login(
    State(state): State<AppState>,
    Json(payload): Json<UserLogin>,
) -> Result<Json<TokenResponse>, ApiError> {
    validate_isu(&payload.isu)?;

    let rate_key = format!("rl:login:{}", payload.isu);
    let allowed = state.redis().rate_limit(&rate_key, AUTH_RATE_LIMIT, AUTH_RATE_WINDOW_SECONDS)
        .await
        .unwrap_or(true);
    if !allowed {
        return Err(ApiError::TooManyRequests("Too many login attempts, try again later"));
    }

    let user = fetch_user_by_isu(&state, &payload.isu).await?;

    let verified = security::verify_password(&payload.password, &user.hashed_password)
        .map_err(|_| ApiError::Unauthorized("Incorrect ISU or password"))?;

    if !verified {
        return Err(ApiError::Unauthorized("Incorrect ISU or password"));
    }

    if !user.is_active {
        return Err(ApiError::BadRequest("Inactive user".to_string()));
    }

    let token = security::create_access_token(&user.id, state.settings(), None)
        .map_err(|_| ApiError::Internal("Failed to create access token".to_string()))?;

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "bearer".to_string(),
        user: UserResponse::from_db(user),
    }))
}

async fn token(
    State(state): State<AppState>,
    Form(payload): Form<OAuth2PasswordForm>,
) -> Result<Json<TokenResponse>, ApiError> {
    validate_isu(&payload.username)?;

    let rate_key = format!("rl:token:{}", payload.username);
    let allowed = state.redis().rate_limit(&rate_key, AUTH_RATE_LIMIT, AUTH_RATE_WINDOW_SECONDS)
        .await
        .unwrap_or(true);
    if !allowed {
        return Err(ApiError::TooManyRequests("Too many token attempts, try again later"));
    }

    let user = fetch_user_by_isu(&state, &payload.username).await?;

    let verified = security::verify_password(&payload.password, &user.hashed_password)
        .map_err(|_| ApiError::Unauthorized("Incorrect ISU or password"))?;

    if !verified {
        return Err(ApiError::Unauthorized("Incorrect ISU or password"));
    }

    if !user.is_active {
        return Err(ApiError::BadRequest("Inactive user".to_string()));
    }

    let token = security::create_access_token(&user.id, state.settings(), None)
        .map_err(|_| ApiError::Internal("Failed to create access token".to_string()))?;

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "bearer".to_string(),
        user: UserResponse::from_db(user),
    }))
}

async fn me(CurrentUser(user): CurrentUser) -> Json<UserResponse> {
    Json(UserResponse::from_db(user))
}

async fn fetch_user_by_isu(state: &AppState, isu: &str) -> Result<User, ApiError> {
    sqlx::query_as::<_, User>(
        "SELECT id, isu, hashed_password, full_name, role, is_active, is_verified,
                pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at,
                terms_version, privacy_version, created_at, updated_at
         FROM users WHERE isu = $1",
    )
    .bind(isu)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to load user".to_string()))?
    .ok_or(ApiError::Unauthorized("Incorrect ISU or password"))
}

fn validate_isu(isu: &str) -> Result<(), ApiError> {
    let valid = isu.len() == 6 && isu.chars().all(|c| c.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(ApiError::BadRequest("Invalid ISU format".to_string()))
    }
}

fn primitive_now_utc(offset: OffsetDateTime) -> PrimitiveDateTime {
    PrimitiveDateTime::new(offset.date(), offset.time())
}
