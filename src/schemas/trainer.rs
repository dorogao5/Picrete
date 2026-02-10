use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct TrainerFilters {
    #[serde(default)]
    pub(crate) paragraph: Option<String>,
    #[serde(default)]
    pub(crate) topic: Option<String>,
    #[serde(default)]
    pub(crate) has_answer: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct TrainerGenerateRequest {
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) filters: TrainerFilters,
    #[validate(range(min = 1, max = 100, message = "count must be in range 1..100"))]
    pub(crate) count: i64,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) seed: Option<u64>,
}

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct TrainerManualCreateRequest {
    pub(crate) source: String,
    #[validate(length(min = 1, max = 300, message = "numbers must contain 1..300 items"))]
    pub(crate) numbers: Vec<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TrainerSetItemImageResponse {
    pub(crate) id: String,
    pub(crate) thumbnail_url: String,
    pub(crate) full_url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct TrainerSetItemResponse {
    pub(crate) id: String,
    pub(crate) number: String,
    pub(crate) paragraph: String,
    pub(crate) topic: String,
    pub(crate) text: String,
    pub(crate) has_answer: bool,
    pub(crate) answer: Option<String>,
    pub(crate) images: Vec<TrainerSetItemImageResponse>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TrainerSetSummaryResponse {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) source: String,
    pub(crate) source_title: String,
    pub(crate) filters: serde_json::Value,
    pub(crate) item_count: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct TrainerSetResponse {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) source: String,
    pub(crate) source_title: String,
    pub(crate) filters: serde_json::Value,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) items: Vec<TrainerSetItemResponse>,
}
