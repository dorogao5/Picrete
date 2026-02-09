use sqlx::types::Json;
use time::PrimitiveDateTime;

use crate::db::types::{LlmPrecheckStatus, OcrOverallStatus, SubmissionStatus};
use std::collections::HashMap;

pub(crate) const COLUMNS: &str = "\
    id, course_id, session_id, student_id, submitted_at, status, ocr_overall_status, \
    llm_precheck_status, report_flag, report_summary, ai_score, final_score, max_score, \
    ai_analysis, ai_comments, ocr_error, ocr_retry_count, ocr_started_at, ocr_completed_at, \
    ai_processed_at, ai_request_started_at, ai_request_completed_at, ai_request_duration_seconds, \
    ai_error, ai_retry_count, teacher_comments, reviewed_by, reviewed_at, is_flagged, \
    flag_reasons, anomaly_scores, files_hash, created_at, updated_at";

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct TeacherSubmissionDetails {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) session_id: String,
    pub(crate) student_id: String,
    pub(crate) submitted_at: PrimitiveDateTime,
    pub(crate) status: SubmissionStatus,
    pub(crate) ocr_overall_status: OcrOverallStatus,
    pub(crate) llm_precheck_status: LlmPrecheckStatus,
    pub(crate) report_flag: bool,
    pub(crate) report_summary: Option<String>,
    pub(crate) ai_score: Option<f64>,
    pub(crate) final_score: Option<f64>,
    pub(crate) max_score: f64,
    pub(crate) ai_analysis: Option<Json<serde_json::Value>>,
    pub(crate) ai_comments: Option<String>,
    pub(crate) ocr_error: Option<String>,
    pub(crate) ai_error: Option<String>,
    pub(crate) teacher_comments: Option<String>,
    pub(crate) is_flagged: bool,
    pub(crate) flag_reasons: Json<Vec<String>>,
    pub(crate) reviewed_by: Option<String>,
    pub(crate) reviewed_at: Option<PrimitiveDateTime>,
    pub(crate) exam_id: String,
    pub(crate) exam_title: String,
    pub(crate) variant_assignments: Json<HashMap<String, String>>,
    pub(crate) student_name: String,
    pub(crate) student_username: String,
}

pub(crate) struct PreliminaryUpdate {
    pub(crate) ai_score: Option<f64>,
    pub(crate) ai_analysis: serde_json::Value,
    pub(crate) ai_comments: Option<String>,
    pub(crate) completed_at: PrimitiveDateTime,
    pub(crate) duration_seconds: f64,
}
