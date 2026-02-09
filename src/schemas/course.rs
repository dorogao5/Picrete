use serde::{Deserialize, Serialize};

use crate::core::time::format_primitive;
use crate::db::types::{CourseRole, MembershipStatus};

#[derive(Debug, Deserialize)]
pub(crate) struct CourseCreate {
    pub(crate) slug: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) organization: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CourseUpdate {
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) organization: Option<String>,
    #[serde(default)]
    pub(crate) is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CourseResponse {
    pub(crate) id: String,
    pub(crate) slug: String,
    pub(crate) title: String,
    pub(crate) organization: Option<String>,
    pub(crate) is_active: bool,
    pub(crate) created_by: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

impl CourseResponse {
    pub(crate) fn from_db(course: crate::db::models::Course) -> Self {
        Self {
            id: course.id,
            slug: course.slug,
            title: course.title,
            organization: course.organization,
            is_active: course.is_active,
            created_by: course.created_by,
            created_at: format_primitive(course.created_at),
            updated_at: format_primitive(course.updated_at),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct MembershipResponse {
    pub(crate) membership_id: String,
    pub(crate) course_id: String,
    pub(crate) course_slug: String,
    pub(crate) course_title: String,
    pub(crate) status: MembershipStatus,
    pub(crate) joined_at: String,
    pub(crate) roles: Vec<CourseRole>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InviteRotateRequest {
    pub(crate) role: CourseRole,
}

#[derive(Debug, Serialize)]
pub(crate) struct InviteCodeResponse {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) role: CourseRole,
    pub(crate) invite_code: String,
    pub(crate) expires_at: Option<String>,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JoinCourseRequest {
    pub(crate) invite_code: String,
    #[serde(default)]
    pub(crate) identity_payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct JoinCourseResponse {
    pub(crate) membership: MembershipResponse,
}

#[derive(Debug, Deserialize)]
pub(crate) struct IdentityPolicyUpdateRequest {
    pub(crate) rule_type: String,
    #[serde(default)]
    pub(crate) rule_config: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct IdentityPolicyResponse {
    pub(crate) course_id: String,
    pub(crate) rule_type: String,
    pub(crate) rule_config: serde_json::Value,
    pub(crate) updated_at: String,
}
