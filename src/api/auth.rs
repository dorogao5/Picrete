use axum::{
    extract::{Form, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::CurrentUser;
use crate::api::validation::{validate_password_len, validate_username};
use crate::core::security;
use crate::core::state::AppState;
use crate::core::time::primitive_now_utc;
use crate::db::models::User;
use crate::repositories;
use crate::schemas::auth::{AuthMeResponse, TokenResponse};
use crate::schemas::course::MembershipResponse;
use crate::schemas::user::{UserCreate, UserLogin, UserResponse};
use crate::services::{invite_codes, membership_policy};

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
    validate_username(&payload.username)?;
    validate_password_len(&payload.password)?;

    let rate_key = format!("rl:signup:{}", payload.username);
    let allowed = match state
        .redis()
        .rate_limit(&rate_key, AUTH_RATE_LIMIT, AUTH_RATE_WINDOW_SECONDS)
        .await
    {
        Ok(value) => value,
        Err(err) => {
            tracing::error!(error = %err, "Failed to check signup rate limit");
            false
        }
    };
    if !allowed {
        return Err(ApiError::TooManyRequests("Too many signup attempts, try again later"));
    }

    if !payload.pd_consent {
        return Err(ApiError::BadRequest("Personal data consent is required".to_string()));
    }

    let existing = repositories::users::exists_by_username(state.db(), &payload.username)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to check existing user"))?;

    if existing.is_some() {
        return Err(ApiError::Conflict("User with this username already exists".to_string()));
    }

    let hashed_password = security::hash_password(&payload.password)
        .map_err(|e| ApiError::internal(e, "Failed to hash password"))?;

    let now_offset = OffsetDateTime::now_utc();
    let now_primitive = primitive_now_utc();

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
            username: &payload.username,
            hashed_password,
            full_name: &payload.full_name,
            is_platform_admin: false,
            is_active: true,
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

    if let Some(invite_code) = payload.invite_code.as_deref() {
        join_with_invite(
            &state,
            &user,
            invite_code,
            payload.identity_payload.clone(),
            now_primitive,
        )
        .await?;
    }

    let memberships = load_memberships(&state, &user).await?;
    let active_course_id = memberships.first().map(|membership| membership.course_id.clone());

    let token = security::create_access_token(&user.id, state.settings(), None)
        .map_err(|e| ApiError::internal(e, "Failed to create access token"))?;

    let response = TokenResponse {
        access_token: token,
        token_type: "bearer".to_string(),
        user: UserResponse::from_db(user),
        memberships,
        active_course_id,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

async fn login(
    State(state): State<AppState>,
    Json(payload): Json<UserLogin>,
) -> Result<Json<TokenResponse>, ApiError> {
    validate_username(&payload.username)?;
    validate_password_len(&payload.password)?;

    let rate_key = format!("rl:login:{}", payload.username);
    let allowed = match state
        .redis()
        .rate_limit(&rate_key, AUTH_RATE_LIMIT, AUTH_RATE_WINDOW_SECONDS)
        .await
    {
        Ok(value) => value,
        Err(err) => {
            tracing::error!(error = %err, "Failed to check login rate limit");
            false
        }
    };
    if !allowed {
        return Err(ApiError::TooManyRequests("Too many login attempts, try again later"));
    }

    let user = fetch_user_by_username(&state, &payload.username).await?;

    let verified = security::verify_password(&payload.password, &user.hashed_password)
        .map_err(|_| ApiError::Unauthorized("Incorrect username or password"))?;

    if !verified {
        return Err(ApiError::Unauthorized("Incorrect username or password"));
    }

    if !user.is_active {
        return Err(ApiError::BadRequest("Inactive user".to_string()));
    }

    let token = security::create_access_token(&user.id, state.settings(), None)
        .map_err(|e| ApiError::internal(e, "Failed to create access token"))?;

    let memberships = load_memberships(&state, &user).await?;
    let active_course_id = memberships.first().map(|membership| membership.course_id.clone());

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "bearer".to_string(),
        user: UserResponse::from_db(user),
        memberships,
        active_course_id,
    }))
}

async fn token(
    State(state): State<AppState>,
    Form(payload): Form<OAuth2PasswordForm>,
) -> Result<Json<TokenResponse>, ApiError> {
    validate_username(&payload.username)?;
    validate_password_len(&payload.password)?;

    let rate_key = format!("rl:token:{}", payload.username);
    let allowed = match state
        .redis()
        .rate_limit(&rate_key, AUTH_RATE_LIMIT, AUTH_RATE_WINDOW_SECONDS)
        .await
    {
        Ok(value) => value,
        Err(err) => {
            tracing::error!(error = %err, "Failed to check token rate limit");
            false
        }
    };
    if !allowed {
        return Err(ApiError::TooManyRequests("Too many token attempts, try again later"));
    }

    let user = fetch_user_by_username(&state, &payload.username).await?;

    let verified = security::verify_password(&payload.password, &user.hashed_password)
        .map_err(|_| ApiError::Unauthorized("Incorrect username or password"))?;

    if !verified {
        return Err(ApiError::Unauthorized("Incorrect username or password"));
    }

    if !user.is_active {
        return Err(ApiError::BadRequest("Inactive user".to_string()));
    }

    let token = security::create_access_token(&user.id, state.settings(), None)
        .map_err(|e| ApiError::internal(e, "Failed to create access token"))?;

    let memberships = load_memberships(&state, &user).await?;
    let active_course_id = memberships.first().map(|membership| membership.course_id.clone());

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "bearer".to_string(),
        user: UserResponse::from_db(user),
        memberships,
        active_course_id,
    }))
}

async fn me(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Json<AuthMeResponse>, ApiError> {
    let memberships = load_memberships(&state, &user).await?;
    let active_course_id = memberships.first().map(|membership| membership.course_id.clone());
    Ok(Json(AuthMeResponse { user: UserResponse::from_db(user), memberships, active_course_id }))
}

async fn fetch_user_by_username(state: &AppState, username: &str) -> Result<User, ApiError> {
    repositories::users::find_by_username(state.db(), username)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load user"))?
        .ok_or(ApiError::Unauthorized("Incorrect username or password"))
}

async fn load_memberships(
    state: &AppState,
    user: &User,
) -> Result<Vec<MembershipResponse>, ApiError> {
    let memberships = repositories::course_memberships::list_for_user(state.db(), &user.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch memberships"))?;

    Ok(memberships
        .into_iter()
        .map(|membership| MembershipResponse {
            membership_id: membership.membership_id,
            course_id: membership.course_id,
            course_slug: membership.course_slug,
            course_title: membership.course_title,
            status: membership.status,
            joined_at: crate::core::time::format_primitive(membership.joined_at),
            roles: membership.roles,
        })
        .collect())
}

async fn join_with_invite(
    state: &AppState,
    user: &User,
    invite_code: &str,
    identity_payload: serde_json::Value,
    now: time::PrimitiveDateTime,
) -> Result<(), ApiError> {
    let code_hash = invite_codes::hash_invite_code(invite_code);
    let invite = repositories::course_invites::find_active_by_hash(state.db(), &code_hash, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load invite code"))?
        .ok_or_else(|| ApiError::BadRequest("Invalid invite code".to_string()))?;

    let policy = repositories::courses::find_identity_policy(state.db(), &invite.course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load identity policy"))?
        .ok_or_else(|| ApiError::Internal("Identity policy is missing for course".to_string()))?;

    membership_policy::validate_identity_payload(
        &policy.rule_type,
        &policy.rule_config.0,
        &identity_payload,
    )
    .map_err(ApiError::UnprocessableEntity)?;

    repositories::course_memberships::ensure_membership_with_role(
        state.db(),
        repositories::course_memberships::EnsureMembershipParams {
            course_id: &invite.course_id,
            user_id: &user.id,
            invited_by: None,
            identity_payload,
            role: invite.role,
            joined_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to upsert membership"))?;

    repositories::course_invites::increment_usage(state.db(), &invite.id, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to update invite usage"))?;

    Ok(())
}
