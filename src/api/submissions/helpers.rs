use std::collections::HashMap;
use time::{OffsetDateTime, PrimitiveDateTime};
use uuid::Uuid;

use crate::api::errors::ApiError;
pub(crate) use crate::core::time::primitive_now_utc as now_primitive;
use crate::db::models::{Exam, ExamSession, Submission, TaskType};
use crate::db::types::{SessionStatus, SubmissionStatus};
use crate::repositories;
use crate::schemas::submission::{
    format_primitive, ExamSessionResponse, SubmissionImageResponse, SubmissionResponse,
    SubmissionScoreResponse,
};

pub(crate) fn session_to_response(session: ExamSession) -> ExamSessionResponse {
    ExamSessionResponse {
        id: session.id,
        exam_id: session.exam_id,
        student_id: session.student_id,
        variant_seed: session.variant_seed,
        variant_assignments: serde_json::to_value(&session.variant_assignments.0)
            .unwrap_or_else(|_| serde_json::json!({})),
        started_at: format_primitive(session.started_at),
        submitted_at: session.submitted_at.map(format_primitive),
        expires_at: format_primitive(session.expires_at),
        status: session.status,
        attempt_number: session.attempt_number,
    }
}

pub(crate) fn to_submission_response(
    submission: Submission,
    images: Vec<SubmissionImageResponse>,
    scores: Vec<SubmissionScoreResponse>,
) -> SubmissionResponse {
    SubmissionResponse {
        id: submission.id,
        session_id: submission.session_id,
        student_id: submission.student_id,
        submitted_at: format_primitive(submission.submitted_at),
        status: submission.status,
        ai_score: submission.ai_score,
        final_score: submission.final_score,
        max_score: submission.max_score,
        ai_analysis: submission.ai_analysis.map(|value| value.0),
        ai_comments: submission.ai_comments,
        teacher_comments: submission.teacher_comments,
        is_flagged: submission.is_flagged,
        flag_reasons: submission.flag_reasons.0,
        reviewed_by: submission.reviewed_by,
        reviewed_at: submission.reviewed_at.map(format_primitive),
        images,
        scores,
    }
}

pub(crate) async fn fetch_exam(pool: &sqlx::PgPool, exam_id: &str) -> Result<Exam, ApiError> {
    repositories::exams::find_by_id(pool, exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?
        .ok_or_else(|| ApiError::NotFound("Exam not found".to_string()))
}

pub(crate) async fn fetch_session(
    pool: &sqlx::PgPool,
    session_id: &str,
) -> Result<ExamSession, ApiError> {
    repositories::sessions::find_by_id(pool, session_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch session"))?
        .ok_or_else(|| ApiError::NotFound("Session not found".to_string()))
}

pub(crate) async fn fetch_task_types(
    pool: &sqlx::PgPool,
    exam_id: &str,
) -> Result<Vec<TaskType>, ApiError> {
    repositories::task_types::list_by_exam(pool, exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch task types"))
}

pub(crate) async fn fetch_images(
    pool: &sqlx::PgPool,
    submission_id: &str,
) -> Result<Vec<SubmissionImageResponse>, ApiError> {
    let images = repositories::images::list_by_submission(pool, submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch images"))?;

    Ok(images
        .into_iter()
        .map(|image| SubmissionImageResponse {
            id: image.id,
            filename: image.filename,
            order_index: image.order_index,
            file_size: image.file_size,
            mime_type: image.mime_type,
            is_processed: image.is_processed,
            quality_score: image.quality_score,
            uploaded_at: format_primitive(image.uploaded_at),
        })
        .collect())
}

pub(crate) async fn fetch_scores(
    pool: &sqlx::PgPool,
    submission_id: &str,
) -> Result<Vec<SubmissionScoreResponse>, ApiError> {
    let scores = repositories::scores::list_by_submission(pool, submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch scores"))?;

    Ok(scores
        .into_iter()
        .map(|score| SubmissionScoreResponse {
            id: score.id,
            submission_id: score.submission_id,
            task_type_id: score.task_type_id,
            criterion_name: score.criterion_name,
            criterion_description: score.criterion_description,
            ai_score: score.ai_score,
            final_score: score.final_score,
            ai_comment: score.ai_comment,
            teacher_comment: score.teacher_comment,
            max_score: score.max_score,
        })
        .collect())
}

pub(crate) async fn build_task_context_from_assignments(
    pool: &sqlx::PgPool,
    exam_id: &str,
    assignments: &HashMap<String, String>,
) -> Result<Vec<serde_json::Value>, ApiError> {
    let task_types = fetch_task_types(pool, exam_id).await?;
    let mut tasks = Vec::new();

    for task_type in task_types {
        let variants = repositories::task_types::list_variants(pool, &task_type.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch variants"))?;

        if let Some(variant_id) = assignments.get(&task_type.id) {
            if let Some(variant) = variants.into_iter().find(|v| &v.id == variant_id) {
                tasks.push(serde_json::json!({
                    "task_type": {
                        "id": task_type.id,
                        "title": task_type.title,
                        "description": task_type.description,
                        "order_index": task_type.order_index,
                        "max_score": task_type.max_score,
                        "formulas": task_type.formulas.0,
                        "units": task_type.units.0,
                    },
                    "variant": {
                        "id": variant.id,
                        "content": variant.content,
                        "parameters": variant.parameters.0,
                        "attachments": variant.attachments.0,
                    }
                }));
            }
        }
    }

    Ok(tasks)
}

pub(crate) async fn enforce_deadline(
    session: &ExamSession,
    pool: &sqlx::PgPool,
) -> Result<(PrimitiveDateTime, SessionStatus), ApiError> {
    let exam = fetch_exam(pool, &session.exam_id).await?;
    let hard_deadline =
        if exam.end_time < session.expires_at { exam.end_time } else { session.expires_at };

    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp()
        && session.status == SessionStatus::Active
    {
        repositories::sessions::update_status(pool, &session.id, SessionStatus::Expired)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to update session status"))?;
        return Ok((hard_deadline, SessionStatus::Expired));
    }

    Ok((hard_deadline, session.status))
}

pub(crate) fn sanitized_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
        .collect();

    if sanitized.is_empty() {
        "upload".to_string()
    } else {
        sanitized
    }
}

pub(crate) async fn ensure_submission(
    pool: &sqlx::PgPool,
    session: &ExamSession,
) -> Result<String, ApiError> {
    let existing_id = repositories::submissions::find_id_by_session(pool, &session.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    if let Some(id) = existing_id {
        return Ok(id);
    }

    let max_score = repositories::exams::max_score_for_exam(pool, &session.exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch max score"))?;

    let now = now_primitive();
    let id = Uuid::new_v4().to_string();
    repositories::submissions::create_if_absent(
        pool,
        &id,
        &session.id,
        &session.student_id,
        SubmissionStatus::Uploaded,
        max_score,
        now,
        now,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create submission"))?;

    repositories::submissions::find_id_by_session(pool, &session.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?
        .ok_or_else(|| ApiError::Internal("Submission missing after creation".to_string()))
}
