use serde::{Deserialize, Serialize};
use validator::Validate;

pub(crate) use crate::core::time::format_primitive;
use crate::db::types::{
    LlmPrecheckStatus, OcrImageStatus, OcrIssueSeverity, OcrOverallStatus, OcrPageStatus,
    SessionStatus, SubmissionStatus,
};

#[derive(Debug, Serialize)]
pub(crate) struct ExamSessionResponse {
    pub(crate) id: String,
    pub(crate) course_id: String,
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
    pub(crate) course_id: String,
    pub(crate) filename: String,
    pub(crate) order_index: i32,
    pub(crate) file_size: i64,
    pub(crate) mime_type: String,
    pub(crate) is_processed: bool,
    pub(crate) ocr_status: OcrImageStatus,
    pub(crate) ocr_text: Option<String>,
    pub(crate) ocr_markdown: Option<String>,
    pub(crate) ocr_chunks: Option<serde_json::Value>,
    pub(crate) quality_score: Option<f64>,
    pub(crate) uploaded_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SubmissionScoreResponse {
    pub(crate) id: String,
    pub(crate) course_id: String,
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
    pub(crate) course_id: String,
    pub(crate) session_id: String,
    pub(crate) student_id: String,
    pub(crate) submitted_at: String,
    pub(crate) status: SubmissionStatus,
    pub(crate) ocr_overall_status: OcrOverallStatus,
    pub(crate) llm_precheck_status: LlmPrecheckStatus,
    pub(crate) report_flag: bool,
    pub(crate) report_summary: Option<String>,
    pub(crate) ocr_error: Option<String>,
    pub(crate) llm_error: Option<String>,
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
    pub(crate) next_step: Option<SubmissionNextStep>,
    pub(crate) images: Vec<SubmissionImageResponse>,
    pub(crate) scores: Vec<SubmissionScoreResponse>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SubmissionNextStep {
    OcrReview,
    Result,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OcrIssueDto {
    pub(crate) anchor: serde_json::Value,
    #[serde(default)]
    pub(crate) original_text: Option<String>,
    #[serde(default)]
    pub(crate) suggested_text: Option<String>,
    pub(crate) note: String,
    #[serde(default = "default_ocr_issue_severity")]
    pub(crate) severity: OcrIssueSeverity,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OcrIssueResponse {
    pub(crate) id: String,
    pub(crate) review_id: String,
    pub(crate) image_id: String,
    pub(crate) anchor: serde_json::Value,
    pub(crate) original_text: Option<String>,
    pub(crate) suggested_text: Option<String>,
    pub(crate) note: String,
    pub(crate) severity: OcrIssueSeverity,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OcrPageResponse {
    pub(crate) image_id: String,
    pub(crate) image_view_url: Option<String>,
    pub(crate) ocr_status: OcrImageStatus,
    pub(crate) ocr_markdown: Option<String>,
    pub(crate) chunks: Option<serde_json::Value>,
    pub(crate) page_status: Option<OcrPageStatus>,
    pub(crate) issues: Vec<OcrIssueResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OcrPagesResponse {
    pub(crate) submission_id: String,
    pub(crate) ocr_status: OcrOverallStatus,
    pub(crate) llm_precheck_status: LlmPrecheckStatus,
    pub(crate) report_flag: bool,
    pub(crate) report_summary: Option<String>,
    pub(crate) pages: Vec<OcrPageResponse>,
}

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct OcrReviewUpsertRequest {
    pub(crate) page_status: OcrPageStatus,
    #[serde(default)]
    pub(crate) issues: Vec<OcrIssueDto>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OcrFinalizeAction {
    Submit,
    Report,
}

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct FinalizeOcrReviewRequest {
    pub(crate) action: OcrFinalizeAction,
    #[serde(default)]
    pub(crate) report_summary: Option<String>,
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

fn default_ocr_issue_severity() -> OcrIssueSeverity {
    OcrIssueSeverity::Major
}
