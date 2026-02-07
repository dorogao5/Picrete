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
use crate::repositories;
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
        .map_err(|e| ApiError::internal(e, "Failed to list users"))?;

    Ok(Json(users.into_iter().map(UserResponse::from_db).collect()))
}

async fn get_user(
    axum::extract::Path(user_id): axum::extract::Path<String>,
    CurrentAdmin(_admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = repositories::users::find_by_id(state.db(), &user_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch user"))?;

    let Some(user) = user else {
        return Err(ApiError::NotFound("User not found".to_string()));
    };

    Ok(Json(UserResponse::from_db(user)))
}

async fn create_user(
    CurrentAdmin(admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
    Json(payload): Json<AdminUserCreate>,
) -> Result<(axum::http::StatusCode, Json<UserResponse>), ApiError> {
    validate_isu(&payload.isu)?;

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

    let user = repositories::users::create(
        state.db(),
        repositories::users::CreateUser {
            id: &Uuid::new_v4().to_string(),
            isu: &payload.isu,
            hashed_password,
            full_name: &payload.full_name,
            role: payload.role,
            is_active: payload.is_active,
            is_verified: payload.is_verified,
            pd_consent: false,
            pd_consent_at: None,
            pd_consent_version: None,
            terms_accepted_at: None,
            terms_version: None,
            privacy_version: None,
            created_at: now_primitive,
            updated_at: now_primitive,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create user"))?;

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
    let user = repositories::users::find_by_id(state.db(), &user_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch user"))?;

    let Some(_user) = user else {
        return Err(ApiError::NotFound("User not found".to_string()));
    };

    let hashed_password = if let Some(password) = payload.password.as_ref() {
        Some(
            security::hash_password(password)
                .map_err(|e| ApiError::internal(e, "Failed to hash password"))?,
        )
    } else {
        None
    };

    let now_offset = OffsetDateTime::now_utc();
    let now_primitive = primitive_now_utc(now_offset);

    repositories::users::update(
        state.db(),
        &user_id,
        repositories::users::UpdateUser {
            full_name: payload.full_name,
            role: payload.role,
            is_active: payload.is_active,
            is_verified: payload.is_verified,
            hashed_password,
            updated_at: now_primitive,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update user"))?;

    let updated = repositories::users::fetch_one_by_id(state.db(), &user_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch updated user"))?;

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
