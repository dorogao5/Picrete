use axum::{extract::Query, routing::get, Json, Router};
use serde::Deserialize;
use time::{OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::{CurrentAdmin, CurrentUser};
use crate::core::security;
use crate::core::state::AppState;
use crate::db::models::User;
use crate::db::types::UserRole;
use crate::schemas::user::{AdminUserCreate, AdminUserUpdate, UserResponse};
use sqlx::{Postgres, QueryBuilder};

#[derive(Debug, Deserialize)]
pub(crate) struct UserListQuery {
    #[serde(default)]
    skip: i64,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    isu: Option<String>,
    #[serde(default)]
    role: Option<UserRole>,
    #[serde(default)]
    #[serde(alias = "isActive")]
    is_active: Option<bool>,
    #[serde(default)]
    #[serde(alias = "isVerified")]
    is_verified: Option<bool>,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(me))
        .route("/", get(list_users).post(create_user))
        .route("/:user_id", get(get_user).patch(update_user))
}

async fn me(CurrentUser(user): CurrentUser) -> Json<UserResponse> {
    Json(UserResponse::from_db(user))
}

async fn list_users(
    Query(params): Query<UserListQuery>,
    CurrentAdmin(_admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
) -> Result<Json<Vec<UserResponse>>, ApiError> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT id, isu, hashed_password, full_name, role, is_active, is_verified,
                pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at,
                terms_version, privacy_version, created_at, updated_at
         FROM users",
    );
    let mut has_where = false;

    if let Some(isu) = params.isu.as_ref() {
        if !has_where {
            builder.push(" WHERE ");
            has_where = true;
        } else {
            builder.push(" AND ");
        }
        builder.push("isu = ");
        builder.push_bind(isu);
    }
    if let Some(role) = params.role {
        if !has_where {
            builder.push(" WHERE ");
            has_where = true;
        } else {
            builder.push(" AND ");
        }
        builder.push("role = ");
        builder.push_bind(role);
    }
    if let Some(is_active) = params.is_active {
        if !has_where {
            builder.push(" WHERE ");
            has_where = true;
        } else {
            builder.push(" AND ");
        }
        builder.push("is_active = ");
        builder.push_bind(is_active);
    }
    if let Some(is_verified) = params.is_verified {
        if !has_where {
            builder.push(" WHERE ");
        } else {
            builder.push(" AND ");
        }
        builder.push("is_verified = ");
        builder.push_bind(is_verified);
    }

    builder.push(" ORDER BY created_at DESC");
    builder.push(" OFFSET ");
    builder.push_bind(params.skip.max(0));
    builder.push(" LIMIT ");
    builder.push_bind(params.limit.clamp(1, 1000));

    let users = builder
        .build_query_as::<User>()
        .fetch_all(state.db())
        .await
        .map_err(|_| ApiError::Internal("Failed to list users".to_string()))?;

    Ok(Json(users.into_iter().map(UserResponse::from_db).collect()))
}

async fn get_user(
    axum::extract::Path(user_id): axum::extract::Path<String>,
    CurrentAdmin(_admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, isu, hashed_password, full_name, role, is_active, is_verified,
                pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at,
                terms_version, privacy_version, created_at, updated_at
         FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch user".to_string()))?;

    let Some(user) = user else {
        return Err(ApiError::BadRequest("User not found".to_string()));
    };

    Ok(Json(UserResponse::from_db(user)))
}

async fn create_user(
    CurrentAdmin(admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
    Json(payload): Json<AdminUserCreate>,
) -> Result<(axum::http::StatusCode, Json<UserResponse>), ApiError> {
    validate_isu(&payload.isu)?;

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

    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (
            id, isu, hashed_password, full_name, role, is_active, is_verified,
            pd_consent, created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        RETURNING id, isu, hashed_password, full_name, role, is_active, is_verified,
            pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at,
            terms_version, privacy_version, created_at, updated_at",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&payload.isu)
    .bind(hashed_password)
    .bind(&payload.full_name)
    .bind(payload.role)
    .bind(payload.is_active)
    .bind(payload.is_verified)
    .bind(false)
    .bind(now_primitive)
    .bind(now_primitive)
    .fetch_one(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to create user".to_string()))?;

    tracing::info!(
        admin_id = %admin.id,
        user_id = %user.id,
        action = "user_create",
        "Admin created user"
    );

    Ok((axum::http::StatusCode::CREATED, Json(UserResponse::from_db(user))))
}

async fn update_user(
    axum::extract::Path(user_id): axum::extract::Path<String>,
    CurrentAdmin(admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
    Json(payload): Json<AdminUserUpdate>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, isu, hashed_password, full_name, role, is_active, is_verified,
                pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at,
                terms_version, privacy_version, created_at, updated_at
         FROM users WHERE id = $1",
    )
    .bind(&user_id)
    .fetch_optional(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch user".to_string()))?;

    let Some(_user) = user else {
        return Err(ApiError::BadRequest("User not found".to_string()));
    };

    let hashed_password = if let Some(password) = payload.password.as_ref() {
        Some(
            security::hash_password(password)
                .map_err(|_| ApiError::Internal("Failed to hash password".to_string()))?,
        )
    } else {
        None
    };

    let now_offset = OffsetDateTime::now_utc();
    let now_primitive = primitive_now_utc(now_offset);

    sqlx::query(
        "UPDATE users SET
            full_name = COALESCE($1, full_name),
            role = COALESCE($2, role),
            is_active = COALESCE($3, is_active),
            is_verified = COALESCE($4, is_verified),
            hashed_password = COALESCE($5, hashed_password),
            updated_at = $6
         WHERE id = $7",
    )
    .bind(payload.full_name)
    .bind(payload.role)
    .bind(payload.is_active)
    .bind(payload.is_verified)
    .bind(hashed_password)
    .bind(now_primitive)
    .bind(&user_id)
    .execute(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to update user".to_string()))?;

    let updated = sqlx::query_as::<_, User>(
        "SELECT id, isu, hashed_password, full_name, role, is_active, is_verified,
                pd_consent, pd_consent_at, pd_consent_version, terms_accepted_at,
                terms_version, privacy_version, created_at, updated_at
         FROM users WHERE id = $1",
    )
    .bind(&user_id)
    .fetch_one(state.db())
    .await
    .map_err(|_| ApiError::Internal("Failed to fetch updated user".to_string()))?;

    tracing::info!(
        admin_id = %admin.id,
        user_id = %updated.id,
        action = "user_update",
        "Admin updated user"
    );

    Ok(Json(UserResponse::from_db(updated)))
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

fn default_limit() -> i64 {
    100
}

#[cfg(test)]
mod tests {
    use super::default_limit;
    use axum::http::{Method, StatusCode};
    use serde_json::json;
    use tower::ServiceExt;

    use crate::db::types::UserRole;
    use crate::test_support;

    #[tokio::test]
    async fn admin_can_create_and_update_user() {
        let ctx = test_support::setup_test_context().await;

        let admin = test_support::insert_user(
            ctx.state.db(),
            "000001",
            "Admin User",
            UserRole::Admin,
            "admin-pass",
        )
        .await;
        let token = test_support::bearer_token(&admin.id, ctx.state.settings());

        let create_payload = json!({
            "isu": "123456",
            "full_name": "Student User",
            "password": "student-pass",
            "role": "student",
            "is_active": true,
            "is_verified": false
        });

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                "/api/v1/users",
                Some(&token),
                Some(create_payload),
            ))
            .await
            .expect("create user");

        let status = response.status();
        let created = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::CREATED, "response: {created}");
        let user_id = created["id"].as_str().expect("user id").to_string();
        assert_eq!(created["isu"], "123456");
        assert_eq!(created["full_name"], "Student User");
        assert_eq!(created["role"], "student");

        let update_payload = json!({
            "full_name": "Updated Student",
            "is_active": false
        });

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::PATCH,
                &format!("/api/v1/users/{user_id}"),
                Some(&token),
                Some(update_payload),
            ))
            .await
            .expect("update user");

        let status = response.status();
        let updated = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {updated}");
        assert_eq!(updated["full_name"], "Updated Student");
        assert_eq!(updated["is_active"], false);

        let response = ctx
            .app
            .oneshot(test_support::json_request(
                Method::GET,
                &format!("/api/v1/users/{user_id}"),
                Some(&token),
                None,
            ))
            .await
            .expect("get user");

        let status = response.status();
        let fetched = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {fetched}");
        assert_eq!(fetched["full_name"], "Updated Student");
    }

    #[test]
    fn default_limit_is_positive() {
        assert!(default_limit() > 0);
    }
}
