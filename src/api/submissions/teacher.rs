use axum::{
    extract::{Path, State},
    Json,
};
use time::OffsetDateTime;

use validator::Validate;

use crate::api::errors::ApiError;
use crate::api::guards::{CurrentTeacher, CurrentUser};
use crate::core::state::AppState;
use crate::db::types::{SubmissionStatus, UserRole};
use crate::repositories;
use crate::schemas::submission::{
    format_primitive, SubmissionApproveRequest, SubmissionOverrideRequest,
};

pub(super) async fn get_submission(
    Path(submission_id): Path<String>,
    CurrentTeacher(_teacher): CurrentTeacher,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let submission = repositories::submissions::find_by_id(state.db(), &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(submission) = submission else {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    };

    let session = super::helpers::fetch_session(state.db(), &submission.session_id).await?;
    let exam = super::helpers::fetch_exam(state.db(), &session.exam_id).await?;

    let student = repositories::users::find_name_by_id(state.db(), &submission.student_id)
        .await
        .unwrap_or(None);
    let student_isu = repositories::users::find_isu_by_id(state.db(), &submission.student_id)
        .await
        .unwrap_or(None);

    let images = super::helpers::fetch_images(state.db(), &submission.id).await?;
    let scores = super::helpers::fetch_scores(state.db(), &submission.id).await?;

    let tasks_payload = super::helpers::build_task_context(state.db(), &session).await?;

    Ok(Json(serde_json::json!({
        "id": submission.id,
        "session_id": submission.session_id,
        "student_id": submission.student_id,
        "submitted_at": format_primitive(submission.submitted_at),
        "status": submission.status,
        "ai_score": submission.ai_score,
        "final_score": submission.final_score,
        "max_score": submission.max_score,
        "ai_analysis": submission.ai_analysis.map(|v| v.0),
        "ai_comments": submission.ai_comments,
        "teacher_comments": submission.teacher_comments,
        "is_flagged": submission.is_flagged,
        "flag_reasons": submission.flag_reasons.0,
        "reviewed_by": submission.reviewed_by,
        "reviewed_at": submission.reviewed_at.map(format_primitive),
        "images": images,
        "scores": scores,
        "student_name": student,
        "student_isu": student_isu,
        "exam": {"id": exam.id, "title": exam.title},
        "tasks": tasks_payload,
    })))
}

pub(super) async fn approve_submission(
    Path(submission_id): Path<String>,
    CurrentTeacher(teacher): CurrentTeacher,
    State(state): State<AppState>,
    Json(payload): Json<SubmissionApproveRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let submission = repositories::submissions::find_by_id(state.db(), &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(submission) = submission else {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    };

    if submission.ai_score.is_none() {
        return Err(ApiError::BadRequest(
            "Cannot approve: AI has not finished grading yet. Use override-score to set a manual score.".to_string()
        ));
    }

    let now = super::helpers::now_primitive();
    repositories::submissions::approve(
        state.db(),
        &submission_id,
        submission.ai_score,
        payload.teacher_comments,
        teacher.id,
        now,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to approve submission"))?;

    Ok(Json(serde_json::json!({"message": "Submission approved"})))
}

pub(super) async fn override_score(
    Path(submission_id): Path<String>,
    CurrentTeacher(teacher): CurrentTeacher,
    State(state): State<AppState>,
    Json(payload): Json<SubmissionOverrideRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let submission = repositories::submissions::find_by_id(state.db(), &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(submission) = submission else {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    };

    if payload.final_score > submission.max_score {
        return Err(ApiError::BadRequest(format!(
            "final_score cannot exceed max_score ({})",
            submission.max_score
        )));
    }

    let now = super::helpers::now_primitive();
    repositories::submissions::override_score(
        state.db(),
        &submission_id,
        payload.final_score,
        payload.teacher_comments,
        teacher.id,
        now,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to override score"))?;

    Ok(Json(serde_json::json!({"message": "Score overridden successfully"})))
}

pub(super) async fn get_image_view_url(
    Path(image_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let image = repositories::images::find_by_id(state.db(), &image_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch image"))?;

    let Some(image) = image else {
        return Err(ApiError::NotFound("Image not found".to_string()));
    };

    let submission = repositories::submissions::fetch_one_by_id(state.db(), &image.submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let is_owner = submission.student_id == user.id;
    let is_teacher = matches!(user.role, UserRole::Teacher | UserRole::Admin);

    if !is_owner && !is_teacher {
        return Err(ApiError::Forbidden("Access denied"));
    }

    if !image.file_path.starts_with("submissions/") {
        return Err(ApiError::BadRequest(
            "Image is stored in local storage. Please migrate to S3 storage.".to_string(),
        ));
    }

    let storage = state
        .storage()
        .ok_or_else(|| ApiError::BadRequest("S3 storage not configured".to_string()))?;

    let url = storage
        .presign_get(&image.file_path, std::time::Duration::from_secs(300))
        .await
        .map_err(|e| ApiError::internal(e, "Failed to generate view URL"))?;

    Ok(Json(serde_json::json!({
        "view_url": url,
        "expires_in": 300,
        "filename": image.filename,
        "mime_type": image.mime_type,
    })))
}

pub(super) async fn regrade_submission(
    Path(submission_id): Path<String>,
    CurrentTeacher(teacher): CurrentTeacher,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let submission = repositories::submissions::find_by_id(state.db(), &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(_submission) = submission else {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    };

    repositories::submissions::queue_regrade(
        state.db(),
        &submission_id,
        super::helpers::now_primitive(),
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update submission"))?;

    tracing::info!(
        teacher_id = %teacher.id,
        submission_id = %submission_id,
        action = "submission_regrade",
        "Submission regrade queued"
    );

    Ok(Json(serde_json::json!({
        "message": "Re-grading queued successfully",
        "submission_id": submission_id,
        "task_id": null,
        "status": "processing"
    })))
}

pub(super) async fn grading_status(
    Path(submission_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let submission = repositories::submissions::find_by_id(state.db(), &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(submission) = submission else {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    };

    let is_owner = submission.student_id == user.id;
    let is_teacher = matches!(user.role, UserRole::Teacher | UserRole::Admin);

    if !is_owner && !is_teacher {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (progress, status_message) = match submission.status {
        SubmissionStatus::Uploaded => (10, "В очереди на проверку"),
        SubmissionStatus::Processing => {
            let mut progress = 50;
            let mut message = "Проверяется ИИ...";
            if let Some(started) = submission.ai_request_started_at {
                let elapsed = OffsetDateTime::now_utc().unix_timestamp()
                    - started.assume_utc().unix_timestamp();
                if elapsed > 120 {
                    progress = 70;
                    message = "Финальная обработка...";
                }
            }
            (progress, message)
        }
        SubmissionStatus::Preliminary => (100, "Проверено ИИ, ожидает подтверждения преподавателя"),
        SubmissionStatus::Approved => (100, "Проверено и одобрено"),
        SubmissionStatus::Flagged => (50, "Требует ручной проверки"),
        SubmissionStatus::Rejected => (50, "Отклонено"),
    };

    Ok(Json(serde_json::json!({
        "submission_id": submission_id,
        "status": submission.status,
        "progress": progress,
        "status_message": status_message,
        "ai_score": submission.ai_score,
        "final_score": submission.final_score,
        "max_score": submission.max_score,
        "ai_comments": submission.ai_comments,
        "ai_error": submission.ai_error,
        "ai_retry_count": submission.ai_retry_count,
        "processing_times": {
            "started_at": submission.ai_request_started_at.map(format_primitive),
            "completed_at": submission.ai_request_completed_at.map(format_primitive),
            "duration_seconds": submission.ai_request_duration_seconds
        }
    })))
}
