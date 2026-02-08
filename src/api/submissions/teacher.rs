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
    CurrentTeacher(teacher): CurrentTeacher,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let details = repositories::submissions::find_teacher_details(state.db(), &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission details"))?;

    let Some(details) = details else {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    };
    if !matches!(teacher.role, UserRole::Admin)
        && details.exam_created_by.as_deref() != Some(teacher.id.as_str())
    {
        return Err(ApiError::Forbidden("You can only manage submissions for your own exams"));
    }

    let images = super::helpers::fetch_images(state.db(), &details.id).await?;
    let scores = super::helpers::fetch_scores(state.db(), &details.id).await?;
    let tasks_payload = super::helpers::build_task_context_from_assignments(
        state.db(),
        &details.exam_id,
        &details.variant_assignments.0,
    )
    .await?;

    Ok(Json(serde_json::json!({
        "id": details.id,
        "session_id": details.session_id,
        "student_id": details.student_id,
        "submitted_at": format_primitive(details.submitted_at),
        "status": details.status,
        "ai_score": details.ai_score,
        "final_score": details.final_score,
        "max_score": details.max_score,
        "ai_analysis": details.ai_analysis.map(|v| v.0),
        "ai_comments": details.ai_comments,
        "teacher_comments": details.teacher_comments,
        "is_flagged": details.is_flagged,
        "flag_reasons": details.flag_reasons.0,
        "reviewed_by": details.reviewed_by,
        "reviewed_at": details.reviewed_at.map(format_primitive),
        "images": images,
        "scores": scores,
        "student_name": details.student_name,
        "student_isu": details.student_isu,
        "exam": {"id": details.exam_id, "title": details.exam_title},
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
    ensure_teacher_can_manage_submission(&state, &teacher, &submission_id).await?;

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
    ensure_teacher_can_manage_submission(&state, &teacher, &submission_id).await?;

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
    if matches!(user.role, UserRole::Teacher) {
        let exam_creator =
            repositories::submissions::find_exam_creator_by_submission(state.db(), &submission.id)
                .await
                .map_err(|e| ApiError::internal(e, "Failed to fetch submission owner"))?
                .ok_or_else(|| ApiError::NotFound("Submission not found".to_string()))?;

        if exam_creator != user.id {
            return Err(ApiError::Forbidden("Access denied"));
        }
    }

    if !image.file_path.starts_with("submissions/") {
        return Err(ApiError::BadRequest(
            "Image is stored in local storage. Please migrate to S3 storage.".to_string(),
        ));
    }

    let storage = state
        .storage()
        .ok_or_else(|| ApiError::ServiceUnavailable("S3 storage not configured".to_string()))?;

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
    ensure_teacher_can_manage_submission(&state, &teacher, &submission_id).await?;

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
    if !is_owner {
        match user.role {
            UserRole::Admin => {}
            UserRole::Teacher => {
                ensure_teacher_can_manage_submission(&state, &user, &submission_id).await?;
            }
            _ => {
                return Err(ApiError::Forbidden("Access denied"));
            }
        }
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

async fn ensure_teacher_can_manage_submission(
    state: &AppState,
    teacher: &crate::db::models::User,
    submission_id: &str,
) -> Result<(), ApiError> {
    if matches!(teacher.role, UserRole::Admin) {
        return Ok(());
    }

    let exam_creator =
        repositories::submissions::find_exam_creator_by_submission(state.db(), submission_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission owner"))?
            .ok_or_else(|| ApiError::NotFound("Submission not found".to_string()))?;

    if exam_creator == teacher.id {
        Ok(())
    } else {
        Err(ApiError::Forbidden("You can only manage submissions for your own exams"))
    }
}
