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
    let mut base = crate::api::submissions::helpers::to_submission_response(
        finalized.submission,
        finalized.images,
        finalized.scores,
    );

    let exam =
        crate::api::submissions::helpers::fetch_exam(state.db(), &course_id, &session.exam_id)
            .await?;
    if !super::feedback_is_released(&session, &exam) {
        super::redact_submission_feedback(&mut base);
    }

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
    let feedback_released = super::feedback_is_released(&session, &exam);
    let ai_analysis =
        feedback_released.then(|| submission.ai_analysis.map(|value| value.0)).flatten();
    let ai_comments = feedback_released.then_some(submission.ai_comments).flatten();
    let scores = feedback_released.then_some(scores).unwrap_or_default();
    let teacher_comments = feedback_released.then_some(submission.teacher_comments).flatten();
    let flag_reasons = feedback_released.then_some(submission.flag_reasons.0).unwrap_or_default();

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
        "report_summary": feedback_released.then_some(submission.report_summary).flatten(),
        "ocr_error": feedback_released.then_some(submission.ocr_error).flatten(),
        "llm_error": feedback_released.then_some(submission.ai_error).flatten(),
        "ai_score": feedback_released.then_some(submission.ai_score).flatten(),
        "final_score": feedback_released.then_some(submission.final_score).flatten(),
        "max_score": submission.max_score,
        "feedback_released": feedback_released,
        "ai_analysis": ai_analysis,
        "ai_comments": ai_comments,
        "teacher_comments": teacher_comments,
        "is_flagged": submission.is_flagged,
        "flag_reasons": flag_reasons,
        "reviewed_by": feedback_released.then_some(submission.reviewed_by).flatten(),
        "reviewed_at": feedback_released
            .then(|| submission.reviewed_at.map(format_primitive))
            .flatten(),
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
