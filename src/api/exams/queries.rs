use serde::Deserialize;

use crate::db::types::{ExamStatus, SubmissionStatus};

#[derive(Debug, Deserialize)]
pub(super) struct ListExamsQuery {
    #[serde(default)]
    pub(super) skip: i64,
    #[serde(default = "crate::api::pagination::default_limit")]
    pub(super) limit: i64,
    #[serde(default)]
    pub(super) status: Option<ExamStatus>,
}

#[derive(Debug, Deserialize)]
pub(super) struct DeleteExamQuery {
    #[serde(default)]
    #[serde(alias = "forceDelete")]
    pub(super) force_delete: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct ListExamSubmissionsQuery {
    #[serde(default)]
    pub(super) status: Option<SubmissionStatus>,
    #[serde(default)]
    pub(super) skip: i64,
    #[serde(default = "crate::api::pagination::default_limit")]
    pub(super) limit: i64,
}
