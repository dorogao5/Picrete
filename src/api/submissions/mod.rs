pub(crate) mod helpers;
mod student;
mod teacher;

use axum::{routing::delete, routing::get, routing::post, Router};
use serde::Deserialize;

use crate::core::state::AppState;
use crate::db::types::SubmissionStatus;

#[derive(Debug, Deserialize)]
pub(crate) struct PresignQuery {
    pub(super) filename: String,
    pub(super) content_type: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListSubmissionsQuery {
    #[serde(default)]
    pub(crate) status: Option<SubmissionStatus>,
    #[serde(default)]
    pub(crate) skip: i64,
    #[serde(default = "crate::api::pagination::default_limit")]
    pub(crate) limit: i64,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        // Student endpoints
        .route("/my-submissions", get(student::get_my_submissions))
        .route("/exams/:exam_id/enter", post(student::enter_exam))
        .route("/sessions/:session_id/variant", get(student::get_session_variant))
        .route("/sessions/:session_id/presigned-upload-url", post(student::presigned_upload_url))
        .route("/sessions/:session_id/upload", post(student::upload_image))
        .route("/sessions/:session_id/images", get(student::list_session_images))
        .route("/sessions/:session_id/images/:image_id", delete(student::delete_session_image))
        .route("/sessions/:session_id/auto-save", post(student::auto_save))
        .route("/sessions/:session_id/submit", post(student::submit_exam))
        .route("/sessions/:session_id/ocr-pages", get(student::get_ocr_pages))
        .route("/sessions/:session_id/ocr-pages/:image_id/review", post(student::review_ocr_page))
        .route("/sessions/:session_id/ocr/finalize", post(student::finalize_ocr_review))
        .route("/sessions/:session_id/result", get(student::get_session_result))
        // Teacher endpoints
        .route("/:submission_id", get(teacher::get_submission))
        .route("/:submission_id/approve", post(teacher::approve_submission))
        .route("/:submission_id/override-score", post(teacher::override_score))
        .route("/images/:image_id/view-url", get(teacher::get_image_view_url))
        .route("/:submission_id/regrade", post(teacher::regrade_submission))
        .route("/grading-status/:submission_id", get(teacher::grading_status))
}

#[cfg(test)]
mod tests;
