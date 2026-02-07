use serde::{Deserialize, Serialize};
use validator::Validate;

pub(crate) use crate::core::time::format_primitive;
use crate::db::types::{SessionStatus, SubmissionStatus};

#[derive(Debug, Serialize)]
pub(crate) struct ExamSessionResponse {
    pub(crate) id: String,
    pub(crate) exam_id: String,
    pub(crate) student_id: String,
    pub(crate) variant_seed: i32,
    pub(crate) variant_assignments: serde_json::Value,
    pub(crate) started_at: String,
    pub(crate) submitted_at: Option<String>,
    pub(crate) expires_at: String,
    pub(crate) status: SessionStatus,
    pub(crate) attempt_number: i32,
}

#[derive(Debug, Serialize)]
pub(crate) struct SubmissionImageResponse {
    pub(crate) id: String,
    pub(crate) filename: String,
    pub(crate) order_index: i32,
    pub(crate) file_size: i64,
    pub(crate) mime_type: String,
    pub(crate) is_processed: bool,
    pub(crate) quality_score: Option<f64>,
    pub(crate) uploaded_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SubmissionScoreResponse {
    pub(crate) id: String,
    pub(crate) submission_id: String,
    pub(crate) task_type_id: String,
    pub(crate) criterion_name: String,
    pub(crate) criterion_description: Option<String>,
    pub(crate) ai_score: Option<f64>,
    pub(crate) final_score: Option<f64>,
    pub(crate) ai_comment: Option<String>,
    pub(crate) teacher_comment: Option<String>,
    pub(crate) max_score: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct SubmissionResponse {
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) student_id: String,
    pub(crate) submitted_at: String,
    pub(crate) status: SubmissionStatus,
    pub(crate) ai_score: Option<f64>,
    pub(crate) final_score: Option<f64>,
    pub(crate) max_score: f64,
    pub(crate) ai_analysis: Option<serde_json::Value>,
    pub(crate) ai_comments: Option<String>,
    pub(crate) teacher_comments: Option<String>,
    pub(crate) is_flagged: bool,
    pub(crate) flag_reasons: Vec<String>,
    pub(crate) reviewed_by: Option<String>,
    pub(crate) reviewed_at: Option<String>,
    pub(crate) images: Vec<SubmissionImageResponse>,
    pub(crate) scores: Vec<SubmissionScoreResponse>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SubmissionApproveRequest {
    #[serde(default)]
    pub(crate) teacher_comments: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct SubmissionOverrideRequest {
    #[validate(range(min = 0.0, message = "final_score must be non-negative"))]
    pub(crate) final_score: f64,
    pub(crate) teacher_comments: String,
}
