use serde::{Deserialize, Serialize};
use sqlx::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "userrole", rename_all = "lowercase")]
pub(crate) enum UserRole {
    Admin,
    Teacher,
    Assistant,
    Student,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "examstatus", rename_all = "lowercase")]
pub(crate) enum ExamStatus {
    Draft,
    Published,
    Active,
    Completed,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "difficultylevel", rename_all = "lowercase")]
pub(crate) enum DifficultyLevel {
    Easy,
    Medium,
    Hard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "sessionstatus", rename_all = "lowercase")]
pub(crate) enum SessionStatus {
    Active,
    Submitted,
    Expired,
    Graded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "submissionstatus", rename_all = "lowercase")]
pub(crate) enum SubmissionStatus {
    Uploaded,
    Processing,
    Preliminary,
    Approved,
    Flagged,
    Rejected,
}
