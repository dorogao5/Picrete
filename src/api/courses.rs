use axum::{routing::get, Json, Router};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_role, CurrentAdmin, CurrentUser};
use crate::core::state::AppState;
use crate::core::time::{format_primitive, primitive_now_utc};
use crate::db::types::CourseRole;
use crate::repositories;
use crate::schemas::course::{
    CourseCreate, CourseResponse, CourseUpdate, IdentityPolicyResponse,
    IdentityPolicyUpdateRequest, InviteCodeResponse, InviteRotateRequest, JoinCourseRequest,
    JoinCourseResponse, MembershipResponse,
};
use crate::services::{invite_codes, membership_policy};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_courses).post(create_course))
        .route("/:course_id", axum::routing::patch(update_course).delete(delete_course))
        .route("/:course_id/invite-codes/rotate", axum::routing::post(rotate_invite_code))
        .route("/:course_id/identity-policy", axum::routing::patch(update_identity_policy))
        .route("/join", axum::routing::post(join_course))
}

async fn create_course(
    CurrentAdmin(admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
    Json(payload): Json<CourseCreate>,
) -> Result<(axum::http::StatusCode, Json<CourseResponse>), ApiError> {
    if payload.slug.trim().is_empty() {
        return Err(ApiError::BadRequest("Course slug must not be empty".to_string()));
    }
    if payload.title.trim().is_empty() {
        return Err(ApiError::BadRequest("Course title must not be empty".to_string()));
    }

    let now = primitive_now_utc();
    let course = repositories::courses::create(
        state.db(),
        repositories::courses::CreateCourse {
            id: &Uuid::new_v4().to_string(),
            slug: payload.slug.trim(),
            title: payload.title.trim(),
            organization: payload.organization.as_deref(),
            is_active: true,
            created_by: &admin.id,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create course"))?;

    repositories::courses::ensure_default_identity_policy(state.db(), &course.id, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to create default identity policy"))?;

    repositories::course_memberships::ensure_membership_with_role(
        state.db(),
        repositories::course_memberships::EnsureMembershipParams {
            course_id: &course.id,
            user_id: &admin.id,
            invited_by: Some(&admin.id),
            identity_payload: serde_json::json!({}),
            role: CourseRole::Teacher,
            joined_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create creator membership"))?;

    Ok((axum::http::StatusCode::CREATED, Json(CourseResponse::from_db(course))))
}

async fn list_courses(
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
) -> Result<Json<Vec<MembershipResponse>>, ApiError> {
    let memberships = repositories::course_memberships::list_for_user(state.db(), &user.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to list memberships"))?;

    let response = memberships
        .into_iter()
        .map(|membership| MembershipResponse {
            membership_id: membership.membership_id,
            course_id: membership.course_id,
            course_slug: membership.course_slug,
            course_title: membership.course_title,
            status: membership.status,
            joined_at: format_primitive(membership.joined_at),
            roles: membership.roles,
        })
        .collect();

    Ok(Json(response))
}

async fn update_course(
    axum::extract::Path(course_id): axum::extract::Path<String>,
    CurrentAdmin(_admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
    Json(payload): Json<CourseUpdate>,
) -> Result<Json<CourseResponse>, ApiError> {
    repositories::courses::update(
        state.db(),
        &course_id,
        repositories::courses::UpdateCourse {
            title: payload.title,
            organization: payload.organization,
            is_active: payload.is_active,
            updated_at: primitive_now_utc(),
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update course"))?;

    let updated = repositories::courses::fetch_one_by_id(state.db(), &course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch updated course"))?;

    Ok(Json(CourseResponse::from_db(updated)))
}

async fn delete_course(
    axum::extract::Path(course_id): axum::extract::Path<String>,
    CurrentAdmin(admin): CurrentAdmin,
    state: axum::extract::State<AppState>,
) -> Result<axum::http::StatusCode, ApiError> {
    let course = repositories::courses::find_by_id(state.db(), &course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch course"))?;

    if course.is_none() {
        return Err(ApiError::NotFound("Course not found".to_string()));
    }

    let deleted = repositories::courses::delete(state.db(), &course_id).await.map_err(|e| {
        if is_foreign_key_violation(&e) {
            ApiError::Conflict("Cannot delete course due dependent records".to_string())
        } else {
            ApiError::internal(e, "Failed to delete course")
        }
    })?;

    if !deleted {
        return Err(ApiError::NotFound("Course not found".to_string()));
    }

    tracing::info!(
        admin_id = %admin.id,
        course_id = %course_id,
        action = "course_delete",
        "Admin deleted course"
    );

    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn rotate_invite_code(
    axum::extract::Path(course_id): axum::extract::Path<String>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Json(payload): Json<InviteRotateRequest>,
) -> Result<Json<InviteCodeResponse>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    let course = repositories::courses::find_by_id(state.db(), &course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch course"))?
        .ok_or_else(|| ApiError::NotFound("Course not found".to_string()))?;

    let now = primitive_now_utc();
    let previous = repositories::course_invites::find_active_for_course_role(
        state.db(),
        &course_id,
        payload.role,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to fetch active invite"))?;

    if let Some(previous) = previous.as_ref() {
        repositories::course_invites::deactivate(state.db(), &previous.id, now)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to deactivate previous invite"))?;
    }

    let invite_code = invite_codes::generate_invite_code(&course.slug, payload.role);
    let code_hash = invite_codes::hash_invite_code(&invite_code);
    let invite = repositories::course_invites::create(
        state.db(),
        repositories::course_invites::CreateInviteCode {
            id: &Uuid::new_v4().to_string(),
            course_id: &course_id,
            role: payload.role,
            code_hash: &code_hash,
            is_active: true,
            rotated_from_id: previous.as_ref().map(|invite| invite.id.as_str()),
            expires_at: None,
            usage_count: 0,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create invite"))?;

    Ok(Json(InviteCodeResponse {
        id: invite.id,
        course_id: invite.course_id,
        role: invite.role,
        invite_code,
        expires_at: invite.expires_at.map(format_primitive),
        created_at: format_primitive(invite.created_at),
    }))
}

async fn update_identity_policy(
    axum::extract::Path(course_id): axum::extract::Path<String>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Json(payload): Json<IdentityPolicyUpdateRequest>,
) -> Result<Json<IdentityPolicyResponse>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    let rule_type = payload.rule_type.trim();
    if !matches!(rule_type, "none" | "isu_6_digits" | "email_domain" | "custom_text_validator") {
        return Err(ApiError::BadRequest("Unsupported identity policy rule_type".to_string()));
    }

    repositories::courses::upsert_identity_policy(
        state.db(),
        &course_id,
        rule_type,
        payload.rule_config,
        primitive_now_utc(),
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update identity policy"))?;

    let policy = repositories::courses::find_identity_policy(state.db(), &course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch identity policy"))?
        .ok_or_else(|| ApiError::Internal("Identity policy missing after update".to_string()))?;

    Ok(Json(IdentityPolicyResponse {
        course_id: policy.course_id,
        rule_type: policy.rule_type,
        rule_config: policy.rule_config.0,
        updated_at: format_primitive(policy.updated_at),
    }))
}

async fn join_course(
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Json(payload): Json<JoinCourseRequest>,
) -> Result<Json<JoinCourseResponse>, ApiError> {
    let invite_code = payload.invite_code.trim();
    if invite_code.is_empty() {
        return Err(ApiError::BadRequest("Invite code must not be empty".to_string()));
    }

    let now = primitive_now_utc();
    let code_hash = invite_codes::hash_invite_code(invite_code);
    let invite = repositories::course_invites::find_active_by_hash(state.db(), &code_hash, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch invite"))?
        .ok_or_else(|| ApiError::BadRequest("Invalid invite code".to_string()))?;

    let policy = repositories::courses::find_identity_policy(state.db(), &invite.course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load identity policy"))?
        .ok_or_else(|| ApiError::Internal("Identity policy is missing for course".to_string()))?;

    membership_policy::validate_identity_payload(
        &policy.rule_type,
        &policy.rule_config.0,
        &payload.identity_payload,
    )
    .map_err(ApiError::UnprocessableEntity)?;

    let membership_id = repositories::course_memberships::ensure_membership_with_role(
        state.db(),
        repositories::course_memberships::EnsureMembershipParams {
            course_id: &invite.course_id,
            user_id: &user.id,
            invited_by: None,
            identity_payload: payload.identity_payload,
            role: invite.role,
            joined_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to upsert membership"))?;

    repositories::course_invites::increment_usage(state.db(), &invite.id, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to increment invite usage"))?;

    let membership = repositories::course_memberships::find_for_user_course(
        state.db(),
        &user.id,
        &invite.course_id,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to fetch joined membership"))?
    .ok_or_else(|| ApiError::Internal("Membership missing after join".to_string()))?;

    Ok(Json(JoinCourseResponse {
        membership: MembershipResponse {
            membership_id,
            course_id: membership.course_id,
            course_slug: membership.course_slug,
            course_title: membership.course_title,
            status: membership.status,
            joined_at: format_primitive(membership.joined_at),
            roles: membership.roles,
        },
    }))
}

fn is_foreign_key_violation(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(db_error) => db_error.code().as_deref() == Some("23503"),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
