use serde::Serialize;

use crate::schemas::course::MembershipResponse;
use crate::schemas::user::UserResponse;

#[derive(Debug, Serialize)]
pub(crate) struct TokenResponse {
    pub(crate) access_token: String,
    pub(crate) token_type: String,
    pub(crate) user: UserResponse,
    pub(crate) memberships: Vec<MembershipResponse>,
    pub(crate) active_course_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuthMeResponse {
    pub(crate) user: UserResponse,
    pub(crate) memberships: Vec<MembershipResponse>,
    pub(crate) active_course_id: Option<String>,
}
