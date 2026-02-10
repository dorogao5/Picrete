use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::FromRow;
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::db::types::{
    CourseRole, DifficultyLevel, ExamStatus, LlmPrecheckStatus, MembershipStatus, OcrImageStatus,
    OcrIssueSeverity, OcrOverallStatus, OcrPageStatus, SessionStatus, SubmissionStatus, WorkKind,
};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct User {
    pub(crate) id: String,
    pub(crate) username: String,
    #[serde(skip_serializing)]
    pub(crate) hashed_password: String,
    pub(crate) full_name: String,
    pub(crate) is_platform_admin: bool,
    pub(crate) is_active: bool,
    pub(crate) pd_consent: bool,
    pub(crate) pd_consent_at: Option<OffsetDateTime>,
    pub(crate) pd_consent_version: Option<String>,
    pub(crate) terms_accepted_at: Option<OffsetDateTime>,
    pub(crate) terms_version: Option<String>,
    pub(crate) privacy_version: Option<String>,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct Course {
    pub(crate) id: String,
    pub(crate) slug: String,
    pub(crate) title: String,
    pub(crate) organization: Option<String>,
    pub(crate) is_active: bool,
    pub(crate) created_by: String,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct CourseMembership {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) user_id: String,
    pub(crate) status: MembershipStatus,
    pub(crate) joined_at: PrimitiveDateTime,
    pub(crate) invited_by: Option<String>,
    pub(crate) identity_payload: Json<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct CourseMembershipRole {
    pub(crate) membership_id: String,
    pub(crate) role: CourseRole,
    pub(crate) granted_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct CourseInviteCode {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) role: CourseRole,
    pub(crate) code_hash: String,
    pub(crate) is_active: bool,
    pub(crate) rotated_from_id: Option<String>,
    pub(crate) expires_at: Option<PrimitiveDateTime>,
    pub(crate) usage_count: i64,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct CourseIdentityPolicy {
    pub(crate) course_id: String,
    pub(crate) rule_type: String,
    pub(crate) rule_config: Json<serde_json::Value>,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct Exam {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) kind: WorkKind,
    pub(crate) start_time: PrimitiveDateTime,
    pub(crate) end_time: PrimitiveDateTime,
    pub(crate) duration_minutes: Option<i32>,
    pub(crate) timezone: String,
    pub(crate) max_attempts: i32,
    pub(crate) allow_breaks: bool,
    pub(crate) break_duration_minutes: i32,
    pub(crate) auto_save_interval: i32,
    pub(crate) status: ExamStatus,
    pub(crate) created_by: Option<String>,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
    pub(crate) published_at: Option<PrimitiveDateTime>,
    pub(crate) settings: Json<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct TaskType {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) exam_id: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) order_index: i32,
    pub(crate) max_score: f64,
    pub(crate) rubric: Json<serde_json::Value>,
    pub(crate) difficulty: DifficultyLevel,
    pub(crate) taxonomy_tags: Json<Vec<String>>,
    pub(crate) formulas: Json<Vec<String>>,
    pub(crate) units: Json<Vec<serde_json::Value>>,
    pub(crate) validation_rules: Json<serde_json::Value>,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct TaskVariant {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) task_type_id: String,
    pub(crate) content: String,
    pub(crate) parameters: Json<serde_json::Value>,
    pub(crate) reference_solution: Option<String>,
    pub(crate) reference_answer: Option<String>,
    pub(crate) answer_tolerance: f64,
    pub(crate) attachments: Json<Vec<String>>,
    pub(crate) created_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct ExamSession {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) exam_id: String,
    pub(crate) student_id: String,
    pub(crate) variant_seed: i32,
    pub(crate) variant_assignments: Json<HashMap<String, String>>,
    pub(crate) started_at: PrimitiveDateTime,
    pub(crate) submitted_at: Option<PrimitiveDateTime>,
    pub(crate) expires_at: PrimitiveDateTime,
    pub(crate) status: SessionStatus,
    pub(crate) attempt_number: i32,
    pub(crate) ip_address: Option<String>,
    pub(crate) user_agent: Option<String>,
    pub(crate) last_auto_save: Option<PrimitiveDateTime>,
    pub(crate) auto_save_data: Json<serde_json::Value>,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct Submission {
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
    pub(crate) ocr_retry_count: i32,
    pub(crate) ocr_started_at: Option<PrimitiveDateTime>,
    pub(crate) ocr_completed_at: Option<PrimitiveDateTime>,
    pub(crate) ai_processed_at: Option<PrimitiveDateTime>,
    pub(crate) ai_request_started_at: Option<PrimitiveDateTime>,
    pub(crate) ai_request_completed_at: Option<PrimitiveDateTime>,
    pub(crate) ai_request_duration_seconds: Option<f64>,
    pub(crate) ai_error: Option<String>,
    pub(crate) ai_retry_count: Option<i32>,
    pub(crate) teacher_comments: Option<String>,
    pub(crate) reviewed_by: Option<String>,
    pub(crate) reviewed_at: Option<PrimitiveDateTime>,
    pub(crate) is_flagged: bool,
    pub(crate) flag_reasons: Json<Vec<String>>,
    pub(crate) anomaly_scores: Json<serde_json::Value>,
    pub(crate) files_hash: Option<String>,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct SubmissionImage {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) submission_id: String,
    pub(crate) filename: String,
    pub(crate) file_path: String,
    pub(crate) file_size: i64,
    pub(crate) mime_type: String,
    pub(crate) is_processed: bool,
    pub(crate) ocr_status: OcrImageStatus,
    pub(crate) ocr_text: Option<String>,
    pub(crate) ocr_markdown: Option<String>,
    pub(crate) ocr_chunks: Option<Json<serde_json::Value>>,
    pub(crate) ocr_model: Option<String>,
    pub(crate) ocr_completed_at: Option<PrimitiveDateTime>,
    pub(crate) ocr_error: Option<String>,
    pub(crate) ocr_request_id: Option<String>,
    pub(crate) quality_score: Option<f64>,
    pub(crate) order_index: i32,
    pub(crate) perceptual_hash: Option<String>,
    pub(crate) uploaded_at: PrimitiveDateTime,
    pub(crate) processed_at: Option<PrimitiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct SubmissionOcrReview {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) submission_id: String,
    pub(crate) image_id: String,
    pub(crate) student_id: String,
    pub(crate) page_status: OcrPageStatus,
    pub(crate) issue_count: i32,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct SubmissionOcrIssue {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) ocr_review_id: String,
    pub(crate) submission_id: String,
    pub(crate) image_id: String,
    pub(crate) anchor: Json<serde_json::Value>,
    pub(crate) original_text: Option<String>,
    pub(crate) suggested_text: Option<String>,
    pub(crate) note: String,
    pub(crate) severity: OcrIssueSeverity,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct SubmissionScore {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) submission_id: String,
    pub(crate) task_type_id: String,
    pub(crate) criterion_name: String,
    pub(crate) criterion_description: Option<String>,
    pub(crate) ai_score: Option<f64>,
    pub(crate) final_score: Option<f64>,
    pub(crate) max_score: f64,
    pub(crate) ai_comment: Option<String>,
    pub(crate) teacher_comment: Option<String>,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct TaskBankSource {
    pub(crate) id: String,
    pub(crate) code: String,
    pub(crate) title: String,
    pub(crate) version: String,
    pub(crate) is_active: bool,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct TaskBankItem {
    pub(crate) id: String,
    pub(crate) source_id: String,
    pub(crate) number: String,
    pub(crate) paragraph: String,
    pub(crate) topic: String,
    pub(crate) text: String,
    pub(crate) answer: Option<String>,
    pub(crate) has_answer: bool,
    pub(crate) metadata: Json<serde_json::Value>,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct TaskBankItemImage {
    pub(crate) id: String,
    pub(crate) task_bank_item_id: String,
    pub(crate) relative_path: String,
    pub(crate) order_index: i32,
    pub(crate) mime_type: String,
    pub(crate) created_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct TrainerSet {
    pub(crate) id: String,
    pub(crate) student_id: String,
    pub(crate) course_id: String,
    pub(crate) title: String,
    pub(crate) source_id: String,
    pub(crate) filters: Json<serde_json::Value>,
    pub(crate) is_deleted: bool,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct TrainerSetItem {
    pub(crate) trainer_set_id: String,
    pub(crate) task_bank_item_id: String,
    pub(crate) order_index: i32,
}
