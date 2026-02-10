use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Serialize)]
pub(crate) struct TaskBankSourceResponse {
    pub(crate) id: String,
    pub(crate) code: String,
    pub(crate) title: String,
    pub(crate) version: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct TaskBankItemImageResponse {
    pub(crate) id: String,
    pub(crate) thumbnail_url: String,
    pub(crate) full_url: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct TaskBankItemResponse {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) number: String,
    pub(crate) paragraph: String,
    pub(crate) topic: String,
    pub(crate) text: String,
    pub(crate) has_answer: bool,
    pub(crate) answer: Option<String>,
    pub(crate) images: Vec<TaskBankItemImageResponse>,
}

#[derive(Debug, Deserialize, Validate)]
pub(crate) struct AddBankTaskToWorkRequest {
    #[validate(length(min = 1, max = 300, message = "bank_item_ids must contain 1..300 items"))]
    pub(crate) bank_item_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AddBankTaskToWorkResponse {
    pub(crate) created_task_type_ids: Vec<String>,
    pub(crate) created_count: usize,
}
