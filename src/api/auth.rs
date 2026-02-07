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
use crate::repositories;
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
    let allowed = state
        .redis()
        .rate_limit(&rate_key, AUTH_RATE_LIMIT, AUTH_RATE_WINDOW_SECONDS)
        .await
        .unwrap_or(true);
    if !allowed {
        return Err(ApiError::TooManyRequests("Too many signup attempts, try again later"));
    }

    if !payload.pd_consent {
        return Err(ApiError::BadRequest("Personal data consent is required".to_string()));
    }

    let existing = repositories::users::exists_by_isu(state.db(), &payload.isu)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to check existing user"))?;

    if existing.is_some() {
        return Err(ApiError::Conflict("User with this ISU already exists".to_string()));
    }

    let hashed_password = security::hash_password(&payload.password)
        .map_err(|e| ApiError::internal(e, "Failed to hash password"))?;

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

    let user = repositories::users::create(
        state.db(),
        repositories::users::CreateUser {
            id: &Uuid::new_v4().to_string(),
            isu: &payload.isu,
            hashed_password,
            full_name: &payload.full_name,
            role: UserRole::Student,
            is_active: true,
            is_verified: false,
            pd_consent: true,
            pd_consent_at: Some(now_offset),
            pd_consent_version: Some(pd_consent_version),
            terms_accepted_at: Some(now_offset),
            terms_version: Some(terms_version),
            privacy_version: Some(privacy_version),
            created_at: now_primitive,
            updated_at: now_primitive,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create user"))?;

    let token = security::create_access_token(&user.id, state.settings(), None)
        .map_err(|e| ApiError::internal(e, "Failed to create access token"))?;

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
    let allowed = state
        .redis()
        .rate_limit(&rate_key, AUTH_RATE_LIMIT, AUTH_RATE_WINDOW_SECONDS)
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
        .map_err(|e| ApiError::internal(e, "Failed to create access token"))?;

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
    let allowed = state
        .redis()
        .rate_limit(&rate_key, AUTH_RATE_LIMIT, AUTH_RATE_WINDOW_SECONDS)
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
        .map_err(|e| ApiError::internal(e, "Failed to create access token"))?;

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
    repositories::users::find_by_isu(state.db(), isu)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load user"))?
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
