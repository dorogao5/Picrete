use axum::{
    extract::{Path, State},
    Json,
};
use time::OffsetDateTime;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_role, CurrentUser};
use crate::core::state::AppState;
use crate::db::types::{CourseRole, SessionStatus};
use crate::repositories;
use crate::schemas::submission::{format_primitive, SubmissionResponse};
use crate::services::submission_finalize::{finalize_submission, FinalizeMode};
use crate::services::work_timing::submit_grace_period_seconds;

pub(in crate::api::submissions) async fn submit_exam(
    Path((course_id, session_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<SubmissionResponse>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;
    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status, work_kind) =
        crate::api::submissions::helpers::enforce_deadline(&session, state.db()).await?;
    let now_offset = OffsetDateTime::now_utc();
    let now = crate::api::submissions::helpers::now_primitive();
    let recently_expired = now_offset.unix_timestamp()
        <= hard_deadline.assume_utc().unix_timestamp() + submit_grace_period_seconds(work_kind);

    if session_status != SessionStatus::Active && !recently_expired {
        return Err(ApiError::BadRequest("WORK_DEADLINE_PASSED".to_string()));
    }

    let finalized = finalize_submission(&state, &session, FinalizeMode::ManualSubmit, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to finalize submission"))?;
    let base = crate::api::submissions::helpers::to_submission_response(
        finalized.submission,
        finalized.images,
        finalized.scores,
    );

    tracing::info!(
        course_id = %course_id,
        session_id = %session_id,
        student_id = %user.id,
        next_step = ?finalized.next_step,
        "Submission accepted and next step resolved"
    );

    Ok(Json(crate::api::submissions::helpers::with_next_step(base, finalized.next_step)))
}

pub(in crate::api::submissions) async fn get_session_result(
    Path((course_id, session_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;
    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let submission =
        repositories::submissions::find_by_session(state.db(), &course_id, &session_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(submission) = submission else {
        return Err(ApiError::BadRequest("No submission found for this session".to_string()));
    };

    let attempts = repositories::sessions::count_by_exam_and_student(
        state.db(),
        &course_id,
        &session.exam_id,
        &user.id,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to count attempts"))?;
    let exam =
        crate::api::submissions::helpers::fetch_exam(state.db(), &course_id, &session.exam_id)
            .await?;

    let images =
        crate::api::submissions::helpers::fetch_images(state.db(), &course_id, &submission.id)
            .await?;
    let scores =
        crate::api::submissions::helpers::fetch_scores(state.db(), &course_id, &submission.id)
            .await?;

    Ok(Json(serde_json::json!({
        "id": submission.id,
        "course_id": course_id,
        "session_id": submission.session_id,
        "student_id": submission.student_id,
        "submitted_at": format_primitive(submission.submitted_at),
        "status": submission.status,
        "ocr_overall_status": submission.ocr_overall_status,
        "llm_precheck_status": submission.llm_precheck_status,
        "report_flag": submission.report_flag,
        "report_summary": submission.report_summary,
        "ocr_error": submission.ocr_error,
        "llm_error": submission.ai_error,
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
            "course_id": exam.course_id,
            "title": exam.title,
            "kind": exam.kind,
            "end_time": format_primitive(exam.end_time),
            "max_attempts": exam.max_attempts,
        },
        "session": {
            "id": session.id,
            "attempt_number": session.attempt_number,
            "total_attempts": attempts,
        }
    })))
}
