use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use time::{
    format_description::well_known::Rfc3339, macros::format_description, OffsetDateTime,
    PrimitiveDateTime,
};
use validator::Validate;

pub(crate) use crate::core::time::format_primitive;
use crate::db::types::{DifficultyLevel, ExamStatus};

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct TaskVariantCreate {
    #[validate(length(min = 1, message = "content must not be empty"))]
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) parameters: serde_json::Value,
    #[serde(default)]
    #[serde(alias = "referenceSolution")]
    pub(crate) reference_solution: Option<String>,
    #[serde(default)]
    #[serde(alias = "referenceAnswer")]
    pub(crate) reference_answer: Option<String>,
    #[serde(default = "default_tolerance")]
    #[serde(alias = "answerTolerance")]
    #[validate(range(min = 0.0, message = "answer_tolerance must be non-negative"))]
    pub(crate) answer_tolerance: f64,
    #[serde(default)]
    pub(crate) attachments: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TaskVariantResponse {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) task_type_id: String,
    pub(crate) content: String,
    pub(crate) parameters: serde_json::Value,
    pub(crate) reference_solution: Option<String>,
    pub(crate) reference_answer: Option<String>,
    pub(crate) answer_tolerance: f64,
    pub(crate) attachments: Vec<String>,
    pub(crate) created_at: String,
}

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct TaskTypeCreate {
    #[validate(length(min = 1, message = "title must not be empty"))]
    pub(crate) title: String,
    pub(crate) description: String,
    #[serde(alias = "orderIndex")]
    #[validate(range(min = 0, message = "order_index must be non-negative"))]
    pub(crate) order_index: i32,
    #[serde(alias = "maxScore")]
    #[validate(range(exclusive_min = 0.0, message = "max_score must be positive"))]
    pub(crate) max_score: f64,
    pub(crate) rubric: serde_json::Value,
    #[serde(default = "default_difficulty")]
    pub(crate) difficulty: DifficultyLevel,
    #[serde(default)]
    #[serde(alias = "taxonomyTags")]
    pub(crate) taxonomy_tags: Vec<String>,
    #[serde(default)]
    pub(crate) formulas: Vec<String>,
    #[serde(default)]
    pub(crate) units: Vec<serde_json::Value>,
    #[serde(default)]
    #[serde(alias = "validationRules")]
    pub(crate) validation_rules: serde_json::Value,
    #[serde(default)]
    #[validate(nested)]
    pub(crate) variants: Vec<TaskVariantCreate>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TaskTypeResponse {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) exam_id: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) order_index: i32,
    pub(crate) max_score: f64,
    pub(crate) rubric: serde_json::Value,
    pub(crate) difficulty: DifficultyLevel,
    pub(crate) taxonomy_tags: Vec<String>,
    pub(crate) formulas: Vec<String>,
    pub(crate) units: Vec<serde_json::Value>,
    pub(crate) validation_rules: serde_json::Value,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) variants: Vec<TaskVariantResponse>,
}

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct ExamCreate {
    #[validate(length(min = 1, message = "title must not be empty"))]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(alias = "startTime", deserialize_with = "deserialize_offset_datetime_flexible")]
    pub(crate) start_time: OffsetDateTime,
    #[serde(alias = "endTime", deserialize_with = "deserialize_offset_datetime_flexible")]
    pub(crate) end_time: OffsetDateTime,
    #[serde(alias = "durationMinutes")]
    #[validate(range(min = 1, message = "duration_minutes must be positive"))]
    pub(crate) duration_minutes: i32,
    #[serde(default = "default_timezone")]
    pub(crate) timezone: String,
    #[serde(default = "default_max_attempts")]
    #[serde(alias = "maxAttempts")]
    #[validate(range(min = 1, message = "max_attempts must be positive"))]
    pub(crate) max_attempts: i32,
    #[serde(default)]
    #[serde(alias = "allowBreaks")]
    pub(crate) allow_breaks: bool,
    #[serde(default)]
    #[serde(alias = "breakDurationMinutes")]
    #[validate(range(min = 0, message = "break_duration_minutes must be non-negative"))]
    pub(crate) break_duration_minutes: i32,
    #[serde(default = "default_auto_save_interval")]
    #[serde(alias = "autoSaveInterval")]
    #[validate(range(min = 1, message = "auto_save_interval must be positive"))]
    pub(crate) auto_save_interval: i32,
    #[serde(default = "default_enabled_true", alias = "ocrEnabled")]
    pub(crate) ocr_enabled: bool,
    #[serde(default = "default_enabled_true", alias = "llmPrecheckEnabled")]
    pub(crate) llm_precheck_enabled: bool,
    #[serde(default)]
    pub(crate) settings: serde_json::Value,
    #[serde(default)]
    #[serde(alias = "taskTypes")]
    #[validate(nested)]
    pub(crate) task_types: Vec<TaskTypeCreate>,
}

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct ExamUpdate {
    #[serde(default)]
    #[validate(length(min = 1, message = "title must not be empty"))]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(
        default,
        alias = "startTime",
        deserialize_with = "deserialize_option_offset_datetime_flexible"
    )]
    pub(crate) start_time: Option<OffsetDateTime>,
    #[serde(
        default,
        alias = "endTime",
        deserialize_with = "deserialize_option_offset_datetime_flexible"
    )]
    pub(crate) end_time: Option<OffsetDateTime>,
    #[serde(default)]
    #[serde(alias = "durationMinutes")]
    #[validate(range(min = 1, message = "duration_minutes must be positive"))]
    pub(crate) duration_minutes: Option<i32>,
    #[serde(default, alias = "ocrEnabled")]
    pub(crate) ocr_enabled: Option<bool>,
    #[serde(default, alias = "llmPrecheckEnabled")]
    pub(crate) llm_precheck_enabled: Option<bool>,
    #[serde(default)]
    pub(crate) settings: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ExamResponse {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) start_time: String,
    pub(crate) end_time: String,
    pub(crate) duration_minutes: i32,
    pub(crate) timezone: String,
    pub(crate) max_attempts: i32,
    pub(crate) allow_breaks: bool,
    pub(crate) break_duration_minutes: i32,
    pub(crate) auto_save_interval: i32,
    pub(crate) ocr_enabled: bool,
    pub(crate) llm_precheck_enabled: bool,
    pub(crate) settings: serde_json::Value,
    pub(crate) status: ExamStatus,
    pub(crate) created_by: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) published_at: Option<String>,
    pub(crate) task_types: Vec<TaskTypeResponse>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ExamSummaryResponse {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) title: String,
    pub(crate) start_time: String,
    pub(crate) end_time: String,
    pub(crate) duration_minutes: i32,
    pub(crate) status: ExamStatus,
    pub(crate) task_count: i64,
    pub(crate) student_count: i64,
    pub(crate) pending_count: i64,
}

fn default_tolerance() -> f64 {
    0.01
}

fn default_difficulty() -> DifficultyLevel {
    DifficultyLevel::Medium
}

fn default_timezone() -> String {
    "Europe/Moscow".to_string()
}

fn default_max_attempts() -> i32 {
    1
}

fn default_auto_save_interval() -> i32 {
    10
}

fn default_enabled_true() -> bool {
    true
}

fn parse_offset_datetime_flexible(raw: &str) -> Option<OffsetDateTime> {
    if let Ok(value) = OffsetDateTime::parse(raw, &Rfc3339) {
        return Some(value);
    }

    // Frontend's datetime-local often sends without timezone.
    if raw.len() == 16 && raw.as_bytes().get(10) == Some(&b'T') {
        let candidate = format!("{raw}:00Z");
        if let Ok(value) = OffsetDateTime::parse(&candidate, &Rfc3339) {
            return Some(value);
        }
    }

    if raw.len() == 19 && raw.as_bytes().get(10) == Some(&b'T') {
        let candidate = format!("{raw}Z");
        if let Ok(value) = OffsetDateTime::parse(&candidate, &Rfc3339) {
            return Some(value);
        }
    }

    // Fallback for explicit format "YYYY-MM-DDTHH:MM[:SS]"
    if let Ok(value) =
        PrimitiveDateTime::parse(raw, &format_description!("[year]-[month]-[day]T[hour]:[minute]"))
    {
        return Some(value.assume_utc());
    }
    if let Ok(value) = PrimitiveDateTime::parse(
        raw,
        &format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"),
    ) {
        return Some(value.assume_utc());
    }

    None
}

fn deserialize_offset_datetime_flexible<'de, D>(deserializer: D) -> Result<OffsetDateTime, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    parse_offset_datetime_flexible(&raw)
        .ok_or_else(|| D::Error::custom(format!("invalid datetime: {raw}")))
}

fn deserialize_option_offset_datetime_flexible<'de, D>(
    deserializer: D,
) -> Result<Option<OffsetDateTime>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    match raw {
        Some(value) => parse_offset_datetime_flexible(&value)
            .ok_or_else(|| D::Error::custom(format!("invalid datetime: {value}")))
            .map(Some),
        None => Ok(None),
    }
}
