use serde::{Deserialize, Serialize};
use sqlx::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "courserole", rename_all = "lowercase")]
pub(crate) enum CourseRole {
    Teacher,
    Student,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "membershipstatus", rename_all = "lowercase")]
pub(crate) enum MembershipStatus {
    Active,
    Suspended,
    Left,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "ocrimagestatus", rename_all = "lowercase")]
pub(crate) enum OcrImageStatus {
    Pending,
    Processing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "ocroverallstatus", rename_all = "snake_case")]
pub(crate) enum OcrOverallStatus {
    NotRequired,
    Pending,
    Processing,
    InReview,
    Validated,
    Reported,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "llmprecheckstatus", rename_all = "lowercase")]
pub(crate) enum LlmPrecheckStatus {
    Skipped,
    Queued,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "ocrpagestatus", rename_all = "lowercase")]
pub(crate) enum OcrPageStatus {
    Approved,
    Reported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "ocrissueseverity", rename_all = "lowercase")]
pub(crate) enum OcrIssueSeverity {
    Minor,
    Major,
    Critical,
}
