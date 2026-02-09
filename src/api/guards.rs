use async_trait::async_trait;
use axum::extract::{FromRequestParts, State};
use axum::http::{header, request::Parts};

use crate::api::errors::ApiError;
use crate::core::{security, state::AppState};
use crate::db::models::User;
use crate::db::types::{CourseRole, MembershipStatus};
use crate::repositories;

pub(crate) struct CurrentUser(pub(crate) User);
pub(crate) struct CurrentAdmin(pub(crate) User);

#[derive(Debug, Clone)]
pub(crate) struct CourseAccess {
    pub(crate) roles: Vec<CourseRole>,
}

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
impl FromRequestParts<AppState> for CurrentAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;

        if user.is_platform_admin {
            Ok(CurrentAdmin(user))
        } else {
            Err(ApiError::Forbidden("Admin access required"))
        }
    }
}

pub(crate) async fn require_course_membership(
    state: &AppState,
    user: &User,
    course_id: &str,
) -> Result<CourseAccess, ApiError> {
    if user.is_platform_admin {
        return Ok(CourseAccess { roles: vec![CourseRole::Teacher] });
    }

    let membership =
        repositories::course_memberships::find_for_user_course(state.db(), &user.id, course_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch course membership"))?;

    let Some(membership) = membership else {
        return Err(ApiError::Forbidden("Membership required for this course"));
    };

    if membership.status != MembershipStatus::Active {
        return Err(ApiError::Forbidden("Membership required for this course"));
    }

    Ok(CourseAccess { roles: membership.roles })
}

pub(crate) async fn require_course_role(
    state: &AppState,
    user: &User,
    course_id: &str,
    role: CourseRole,
) -> Result<CourseAccess, ApiError> {
    let access = require_course_membership(state, user, course_id).await?;

    if user.is_platform_admin || access.roles.iter().any(|current| *current == role) {
        return Ok(access);
    }

    Err(ApiError::Forbidden("Not enough permissions for this course"))
}
