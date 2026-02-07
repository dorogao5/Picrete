use axum::{
    extract::{Path, State},
    Json,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::CurrentUser;
use crate::core::state::AppState;
use crate::db::types::{SessionStatus, SubmissionStatus};
use crate::repositories;
use crate::schemas::submission::{format_primitive, SubmissionResponse};

pub(in crate::api::submissions) async fn submit_exam(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<SubmissionResponse>, ApiError> {
    let session = crate::api::submissions::helpers::fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) =
        crate::api::submissions::helpers::enforce_deadline(&session, state.db()).await?;
    let now_offset = OffsetDateTime::now_utc();
    let now = crate::api::submissions::helpers::now_primitive();
    let recently_expired =
        now_offset.unix_timestamp() <= hard_deadline.assume_utc().unix_timestamp() + 300;

    if session_status != SessionStatus::Active && !recently_expired {
        return Err(ApiError::BadRequest("Session is not active or has expired".to_string()));
    }

    // Check image count before creating/updating submission (fail fast, no partial state)
    let existing_submission_id =
        repositories::submissions::find_id_by_session(state.db(), &session_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;
    let images = match &existing_submission_id {
        Some(id) => crate::api::submissions::helpers::fetch_images(state.db(), id).await?,
        None => vec![],
    };
    if images.is_empty() {
        return Err(ApiError::BadRequest(
            "Добавьте хотя бы одно фото решения перед отправкой".to_string(),
        ));
    }

    let max_score = repositories::exams::max_score_for_exam(state.db(), &session.exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch max score"))?;
    let submission_id = Uuid::new_v4().to_string();
    repositories::submissions::create_if_absent(
        state.db(),
        &submission_id,
        &session_id,
        &session.student_id,
        SubmissionStatus::Uploaded,
        max_score,
        now,
        now,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create submission"))?;

    let submission = repositories::submissions::find_by_session(state.db(), &session_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to refresh submission"))?
        .ok_or_else(|| ApiError::Internal("Submission missing".to_string()))?;

    repositories::sessions::submit(state.db(), &session_id, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to update session"))?;

    repositories::submissions::update_status_by_session(
        state.db(),
        &session_id,
        SubmissionStatus::Uploaded,
        now,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update submission"))?;

    let scores = crate::api::submissions::helpers::fetch_scores(state.db(), &submission.id).await?;

    Ok(Json(crate::api::submissions::helpers::to_submission_response(submission, images, scores)))
}

pub(in crate::api::submissions) async fn get_session_result(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = crate::api::submissions::helpers::fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let submission = repositories::submissions::find_by_session(state.db(), &session_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(submission) = submission else {
        return Err(ApiError::BadRequest("No submission found for this session".to_string()));
    };

    let attempts =
        repositories::sessions::count_by_exam_and_student(state.db(), &session.exam_id, &user.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to count attempts"))?;
    let exam = crate::api::submissions::helpers::fetch_exam(state.db(), &session.exam_id).await?;

    let images = crate::api::submissions::helpers::fetch_images(state.db(), &submission.id).await?;
    let scores = crate::api::submissions::helpers::fetch_scores(state.db(), &submission.id).await?;

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
        "exam": {
            "id": session.exam_id,
            "title": exam.title,
            "max_attempts": exam.max_attempts,
        },
        "session": {
            "id": session.id,
            "attempt_number": session.attempt_number,
            "total_attempts": attempts,
        }
    })))
}
