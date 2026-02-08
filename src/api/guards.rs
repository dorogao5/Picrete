use async_trait::async_trait;
use axum::extract::{FromRequestParts, State};
use axum::http::{header, request::Parts};

use crate::api::errors::ApiError;
use crate::core::{security, state::AppState};
use crate::db::models::User;
use crate::db::types::UserRole;
use crate::repositories;

pub(crate) struct CurrentUser(pub(crate) User);
pub(crate) struct CurrentTeacher(pub(crate) User);
pub(crate) struct CurrentAdmin(pub(crate) User);

#[async_trait]
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let State(app_state) = State::<AppState>::from_request_parts(parts, state)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to access application state"))?;

        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(ApiError::Unauthorized("Invalid authentication credentials"))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized("Invalid authentication credentials"))?;

        let claims = security::verify_token(token, app_state.settings())
            .map_err(|_| ApiError::Unauthorized("Invalid authentication credentials"))?;

        let user = repositories::users::find_by_id(app_state.db(), &claims.sub)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to load user"))?;

        let Some(user) = user else {
            return Err(ApiError::Unauthorized("User not found"));
        };

        if !user.is_active {
            return Err(ApiError::Unauthorized("Invalid authentication credentials"));
        }

        Ok(CurrentUser(user))
    }
}

#[async_trait]
impl FromRequestParts<AppState> for CurrentTeacher {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;

        if matches!(user.role, UserRole::Teacher | UserRole::Admin) {
            Ok(CurrentTeacher(user))
        } else {
            Err(ApiError::Forbidden("Not enough permissions"))
        }
    }
}

#[async_trait]
impl FromRequestParts<AppState> for CurrentAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;

        if matches!(user.role, UserRole::Admin) {
            Ok(CurrentAdmin(user))
        } else {
            Err(ApiError::Forbidden("Admin access required"))
        }
    }
}
