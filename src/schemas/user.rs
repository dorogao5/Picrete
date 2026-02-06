use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, PrimitiveDateTime};

use crate::db::types::UserRole;

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

fn format_offset(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).unwrap_or_else(|_| value.to_string())
}

fn format_primitive(value: PrimitiveDateTime) -> String {
    value.assume_utc().format(&Rfc3339).unwrap_or_else(|_| value.assume_utc().to_string())
}

fn default_user_role() -> UserRole {
    UserRole::Student
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Date, Time};

    #[test]
    fn format_primitive_outputs_utc_z() {
        let date = Date::from_calendar_date(2025, time::Month::March, 4).unwrap();
        let time = Time::from_hms(7, 8, 9).unwrap();
        let value = PrimitiveDateTime::new(date, time);
        assert_eq!(format_primitive(value), "2025-03-04T07:08:09Z");
    }
}
