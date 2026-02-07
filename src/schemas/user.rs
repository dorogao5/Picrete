use crate::core::time::{format_offset, format_primitive};
use crate::db::types::UserRole;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct UserCreate {
    pub(crate) isu: String,
    #[serde(alias = "fullName")]
    pub(crate) full_name: String,
    pub(crate) password: String,
    #[serde(default)]
    #[serde(alias = "pdConsent")]
    pub(crate) pd_consent: bool,
    #[serde(default)]
    #[serde(alias = "pdConsentVersion")]
    pub(crate) pd_consent_version: Option<String>,
    #[serde(default)]
    #[serde(alias = "termsVersion")]
    pub(crate) terms_version: Option<String>,
    #[serde(default)]
    #[serde(alias = "privacyVersion")]
    pub(crate) privacy_version: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserLogin {
    pub(crate) isu: String,
    pub(crate) password: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminUserCreate {
    pub(crate) isu: String,
    #[serde(alias = "fullName")]
    pub(crate) full_name: String,
    pub(crate) password: String,
    #[serde(default = "default_user_role")]
    pub(crate) role: UserRole,
    #[serde(default = "default_true")]
    #[serde(alias = "isActive")]
    pub(crate) is_active: bool,
    #[serde(default)]
    #[serde(alias = "isVerified")]
    pub(crate) is_verified: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AdminUserUpdate {
    #[serde(default)]
    #[serde(alias = "fullName")]
    pub(crate) full_name: Option<String>,
    #[serde(default)]
    pub(crate) password: Option<String>,
    #[serde(default)]
    pub(crate) role: Option<UserRole>,
    #[serde(default)]
    #[serde(alias = "isActive")]
    pub(crate) is_active: Option<bool>,
    #[serde(default)]
    #[serde(alias = "isVerified")]
    pub(crate) is_verified: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(crate) struct UserResponse {
    pub(crate) id: String,
    pub(crate) isu: String,
    pub(crate) full_name: String,
    pub(crate) role: UserRole,
    pub(crate) is_active: bool,
    pub(crate) is_verified: bool,
    pub(crate) created_at: String,
    pub(crate) pd_consent: bool,
    pub(crate) pd_consent_at: Option<String>,
    pub(crate) pd_consent_version: Option<String>,
    pub(crate) terms_accepted_at: Option<String>,
    pub(crate) terms_version: Option<String>,
    pub(crate) privacy_version: Option<String>,
}

impl UserResponse {
    pub(crate) fn from_db(user: crate::db::models::User) -> Self {
        Self {
            id: user.id,
            isu: user.isu,
            full_name: user.full_name,
            role: user.role,
            is_active: user.is_active,
            is_verified: user.is_verified,
            created_at: format_primitive(user.created_at),
            pd_consent: user.pd_consent,
            pd_consent_at: user.pd_consent_at.map(format_offset),
            pd_consent_version: user.pd_consent_version,
            terms_accepted_at: user.terms_accepted_at.map(format_offset),
            terms_version: user.terms_version,
            privacy_version: user.privacy_version,
        }
    }
}

fn default_user_role() -> UserRole {
    UserRole::Student
}

fn default_true() -> bool {
    true
}
