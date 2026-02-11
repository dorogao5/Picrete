use std::collections::HashMap;
use time::{OffsetDateTime, PrimitiveDateTime};

use crate::api::errors::ApiError;
pub(crate) use crate::core::time::primitive_now_utc as now_primitive;
use crate::db::models::{Exam, ExamSession, Submission, TaskType, TaskVariant};
use crate::db::types::{SessionStatus, WorkKind};
use crate::repositories;
use crate::schemas::submission::{
    format_primitive, ExamSessionResponse, SubmissionImageResponse, SubmissionNextStep,
    SubmissionResponse, SubmissionScoreResponse,
};

pub(crate) fn session_to_response(session: ExamSession) -> ExamSessionResponse {
    ExamSessionResponse {
        id: session.id,
        course_id: session.course_id,
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
        course_id: submission.course_id,
        session_id: submission.session_id,
        student_id: submission.student_id,
        submitted_at: format_primitive(submission.submitted_at),
        status: submission.status,
        ocr_overall_status: submission.ocr_overall_status,
        llm_precheck_status: submission.llm_precheck_status,
        report_flag: submission.report_flag,
        report_summary: submission.report_summary,
        ocr_error: submission.ocr_error,
        llm_error: submission.ai_error,
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
        next_step: None,
        images,
        scores,
    }
}

pub(crate) fn with_next_step(
    mut response: SubmissionResponse,
    next_step: SubmissionNextStep,
) -> SubmissionResponse {
    response.next_step = Some(next_step);
    response
}

pub(crate) async fn fetch_exam(
    pool: &sqlx::PgPool,
    course_id: &str,
    exam_id: &str,
) -> Result<Exam, ApiError> {
    repositories::exams::find_by_id(pool, course_id, exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?
        .ok_or_else(|| ApiError::NotFound("Exam not found".to_string()))
}

pub(crate) async fn fetch_session(
    pool: &sqlx::PgPool,
    course_id: &str,
    session_id: &str,
) -> Result<ExamSession, ApiError> {
    repositories::sessions::find_by_id(pool, course_id, session_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch session"))?
        .ok_or_else(|| ApiError::NotFound("Session not found".to_string()))
}

pub(crate) async fn fetch_task_types(
    pool: &sqlx::PgPool,
    course_id: &str,
    exam_id: &str,
) -> Result<Vec<TaskType>, ApiError> {
    repositories::task_types::list_by_exam(pool, course_id, exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch task types"))
}

pub(crate) async fn fetch_images(
    pool: &sqlx::PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<Vec<SubmissionImageResponse>, ApiError> {
    let images = repositories::images::list_by_submission(pool, course_id, submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch images"))?;

    Ok(images
        .into_iter()
        .map(|image| SubmissionImageResponse {
            id: image.id,
            course_id: image.course_id,
            filename: image.filename,
            order_index: image.order_index,
            upload_source: image.upload_source,
            file_size: image.file_size,
            mime_type: image.mime_type,
            is_processed: image.is_processed,
            ocr_status: image.ocr_status,
            ocr_text: image.ocr_text,
            ocr_markdown: image.ocr_markdown,
            ocr_chunks: image.ocr_chunks.map(|value| value.0),
            quality_score: image.quality_score,
            uploaded_at: format_primitive(image.uploaded_at),
            view_url: None,
        })
        .collect())
}

pub(crate) async fn fetch_scores(
    pool: &sqlx::PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<Vec<SubmissionScoreResponse>, ApiError> {
    let scores = repositories::scores::list_by_submission(pool, course_id, submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch scores"))?;

    Ok(scores
        .into_iter()
        .map(|score| SubmissionScoreResponse {
            id: score.id,
            course_id: score.course_id,
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
    course_id: &str,
    exam_id: &str,
    assignments: &HashMap<String, String>,
) -> Result<Vec<serde_json::Value>, ApiError> {
    let task_types = fetch_task_types(pool, course_id, exam_id).await?;
    let task_type_ids = task_types.iter().map(|task_type| task_type.id.clone()).collect::<Vec<_>>();
    let variants =
        repositories::task_types::list_variants_by_task_type_ids(pool, course_id, &task_type_ids)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch variants"))?;

    let mut variants_by_task_id = HashMap::<String, HashMap<String, TaskVariant>>::new();
    for variant in variants {
        variants_by_task_id
            .entry(variant.task_type_id.clone())
            .or_default()
            .insert(variant.id.clone(), variant);
    }

    let mut tasks = Vec::new();

    for task_type in task_types {
        if let Some(variant_id) = assignments.get(&task_type.id) {
            if let Some(variant) =
                variants_by_task_id.get(&task_type.id).and_then(|variants| variants.get(variant_id))
            {
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
) -> Result<(PrimitiveDateTime, SessionStatus, WorkKind), ApiError> {
    let exam = fetch_exam(pool, &session.course_id, &session.exam_id).await?;
    let hard_deadline = crate::services::work_timing::compute_hard_deadline(
        exam.kind,
        session.started_at,
        session.expires_at,
        exam.end_time,
        exam.duration_minutes,
    )
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp()
        && session.status == SessionStatus::Active
    {
        repositories::sessions::expire_with_deadline(
            pool,
            &session.course_id,
            &session.id,
            hard_deadline,
            now_primitive(),
        )
        .await
        .map_err(|e| ApiError::internal(e, "Failed to expire session"))?;
        return Ok((hard_deadline, SessionStatus::Expired, exam.kind));
    }

    Ok((hard_deadline, session.status, exam.kind))
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
