use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::FromRow;
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::db::types::{DifficultyLevel, ExamStatus, SessionStatus, SubmissionStatus, UserRole};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct User {
    pub(crate) id: String,
    pub(crate) isu: String,
    pub(crate) hashed_password: String,
    pub(crate) full_name: String,
    pub(crate) role: UserRole,
    pub(crate) is_active: bool,
    pub(crate) is_verified: bool,
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
pub(crate) struct Exam {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) start_time: PrimitiveDateTime,
    pub(crate) end_time: PrimitiveDateTime,
    pub(crate) duration_minutes: i32,
    pub(crate) timezone: String,
    pub(crate) max_attempts: i32,
    pub(crate) allow_breaks: bool,
    pub(crate) break_duration_minutes: i32,
    pub(crate) auto_save_interval: i32,
    pub(crate) status: ExamStatus,
    pub(crate) created_by: String,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
    pub(crate) published_at: Option<PrimitiveDateTime>,
    pub(crate) settings: Json<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct TaskType {
    pub(crate) id: String,
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
    pub(crate) session_id: String,
    pub(crate) student_id: String,
    pub(crate) submitted_at: PrimitiveDateTime,
    pub(crate) status: SubmissionStatus,
    pub(crate) ai_score: Option<f64>,
    pub(crate) final_score: Option<f64>,
    pub(crate) max_score: f64,
    pub(crate) ai_analysis: Option<Json<serde_json::Value>>,
    pub(crate) ai_comments: Option<String>,
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
    pub(crate) submission_id: String,
    pub(crate) filename: String,
    pub(crate) file_path: String,
    pub(crate) file_size: i64,
    pub(crate) mime_type: String,
    pub(crate) is_processed: bool,
    pub(crate) ocr_text: Option<String>,
    pub(crate) quality_score: Option<f64>,
    pub(crate) order_index: i32,
    pub(crate) perceptual_hash: Option<String>,
    pub(crate) uploaded_at: PrimitiveDateTime,
    pub(crate) processed_at: Option<PrimitiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub(crate) struct SubmissionScore {
    pub(crate) id: String,
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
