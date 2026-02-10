use axum::{extract::Query, routing::get, Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::{CurrentAdmin, CurrentUser};
use crate::api::pagination::{default_limit, PaginatedResponse};
use crate::api::validation::{validate_password_len, validate_username};
use crate::core::security;
use crate::core::state::AppState;
use crate::core::time::primitive_now_utc;
use crate::repositories;
use crate::schemas::user::{AdminUserCreate, AdminUserUpdate, UserResponse};

#[derive(Debug, Deserialize)]
pub(crate) struct UserListQuery {
    #[serde(default)]
    skip: i64,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    #[serde(alias = "isPlatformAdmin")]
    is_platform_admin: Option<bool>,
    #[serde(default)]
    #[serde(alias = "isActive")]
    is_active: Option<bool>,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/me", get(me))
        .route("/", get(list_users).post(create_user))
        .route("/:user_id", get(get_user).patch(update_user).delete(delete_user))
}

async fn me(CurrentUser(user): CurrentUser) -> Json<UserResponse> {
    Json(UserResponse::from_db(user))
}

async fn list_users(
    Query(params): Query<UserListQuery>,
    CurrentAdmin(_admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
) -> Result<Json<PaginatedResponse<UserResponse>>, ApiError> {
    let skip = params.skip.max(0);
    let limit = params.limit.clamp(1, 1000);
    let filters = repositories::users::UserListFilters {
        username: params.username.as_deref(),
        is_platform_admin: params.is_platform_admin,
        is_active: params.is_active,
    };

    let users = repositories::users::list(state.db(), filters, skip, limit)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to list users"))?;
    let total_count = repositories::users::count(state.db(), filters)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to count users"))?;

    Ok(Json(PaginatedResponse {
        items: users.into_iter().map(UserResponse::from_db).collect(),
        total_count,
        skip,
        limit,
    }))
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
    validate_username(&payload.username)?;
    validate_password_len(&payload.password)?;

    let existing = repositories::users::exists_by_username(state.db(), &payload.username)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to check existing user"))?;

    if existing.is_some() {
        return Err(ApiError::Conflict("User with this username already exists".to_string()));
    }

    let hashed_password = security::hash_password(&payload.password)
        .map_err(|e| ApiError::internal(e, "Failed to hash password"))?;

    let now_primitive = primitive_now_utc();

    let user = repositories::users::create(
        state.db(),
        repositories::users::CreateUser {
            id: &Uuid::new_v4().to_string(),
            username: &payload.username,
            hashed_password,
            full_name: &payload.full_name,
            is_platform_admin: payload.is_platform_admin,
            is_active: payload.is_active,
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
        validate_password_len(password)?;
        Some(
            security::hash_password(password)
                .map_err(|e| ApiError::internal(e, "Failed to hash password"))?,
        )
    } else {
        None
    };

    let now_primitive = primitive_now_utc();

    repositories::users::update(
        state.db(),
        &user_id,
        repositories::users::UpdateUser {
            full_name: payload.full_name,
            is_platform_admin: payload.is_platform_admin,
            is_active: payload.is_active,
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

async fn delete_user(
    axum::extract::Path(user_id): axum::extract::Path<String>,
    CurrentAdmin(admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
) -> Result<axum::http::StatusCode, ApiError> {
    let user = repositories::users::find_by_id(state.db(), &user_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch user"))?;

    let Some(user) = user else {
        return Err(ApiError::NotFound("User not found".to_string()));
    };

    if user.id == admin.id {
        return Err(ApiError::BadRequest("Cannot delete your own account".to_string()));
    }

    if user.is_platform_admin {
        let admin_count = repositories::users::count(
            state.db(),
            repositories::users::UserListFilters {
                username: None,
                is_platform_admin: Some(true),
                is_active: None,
            },
        )
        .await
        .map_err(|e| ApiError::internal(e, "Failed to count platform admins"))?;

        if admin_count <= 1 {
            return Err(ApiError::Conflict("Cannot delete the last platform admin".to_string()));
        }
    }

    let deleted = repositories::users::delete(state.db(), &user_id).await.map_err(|e| {
        if is_foreign_key_violation(&e) {
            ApiError::Conflict(
                "Cannot delete user with dependent records. Delete owned courses first."
                    .to_string(),
            )
        } else {
            ApiError::internal(e, "Failed to delete user")
        }
    })?;

    if !deleted {
        return Err(ApiError::NotFound("User not found".to_string()));
    }

    tracing::info!(
        admin_id = %admin.id,
        user_id = %user_id,
        action = "user_delete",
        "Admin deleted user"
    );

    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn is_foreign_key_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db_error) => db_error.code().as_deref() == Some("23503"),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
