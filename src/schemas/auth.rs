use serde::Serialize;

use crate::schemas::user::UserResponse;

#[derive(Debug, Serialize)]
pub(crate) struct TokenResponse {
    pub(crate) access_token: String,
    pub(crate) token_type: String,
    pub(crate) user: UserResponse,
}
