use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    Json,
};
use time::OffsetDateTime;
use validator::Validate;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_membership, require_course_role, CurrentUser};
use crate::core::state::AppState;
use crate::db::types::{CourseRole, LlmPrecheckStatus, OcrOverallStatus, SubmissionStatus};
use crate::repositories;
use crate::schemas::submission::{
    format_primitive, SubmissionApproveRequest, SubmissionOverrideRequest,
};
use crate::services::work_processing::WorkProcessingSettings;

pub(super) async fn get_submission(
    Path((course_id, submission_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    let details =
        repositories::submissions::find_teacher_details(state.db(), &course_id, &submission_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission details"))?;

    let Some(details) = details else {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    };

    if !matches!(
        details.status,
        SubmissionStatus::Preliminary
            | SubmissionStatus::Approved
            | SubmissionStatus::Flagged
            | SubmissionStatus::Rejected
    ) {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    }

    let images = super::helpers::fetch_images(state.db(), &course_id, &details.id).await?;
    let scores = super::helpers::fetch_scores(state.db(), &course_id, &details.id).await?;
    let reviews =
        repositories::ocr_reviews::list_reviews_by_submission(state.db(), &course_id, &details.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch OCR reviews"))?;
    let issues =
        repositories::ocr_reviews::list_issues_by_submission(state.db(), &course_id, &details.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch OCR issues"))?;
    let tasks_payload = super::helpers::build_task_context_from_assignments(
        state.db(),
        &course_id,
        &details.exam_id,
        &details.variant_assignments.0,
    )
    .await?;

    let review_by_image: HashMap<String, crate::db::models::SubmissionOcrReview> =
        reviews.into_iter().map(|review| (review.image_id.clone(), review)).collect();
    let mut issues_by_review: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for issue in issues {
        issues_by_review.entry(issue.ocr_review_id.clone()).or_default().push(serde_json::json!({
            "id": issue.id,
            "review_id": issue.ocr_review_id,
            "image_id": issue.image_id,
            "anchor": issue.anchor.0,
            "original_text": issue.original_text,
            "suggested_text": issue.suggested_text,
            "note": issue.note,
            "severity": issue.severity,
            "created_at": format_primitive(issue.created_at),
        }));
    }

    let mut report_issues = Vec::new();
    let mut ocr_pages = Vec::new();
    for image in &images {
        let (page_status, review_issues) = if let Some(review) = review_by_image.get(&image.id) {
            let issues = issues_by_review.remove(&review.id).unwrap_or_default();
            report_issues.extend(issues.clone());
            (Some(review.page_status), issues)
        } else {
            (None, Vec::new())
        };

        ocr_pages.push(serde_json::json!({
            "image_id": image.id,
            "order_index": image.order_index,
            "ocr_status": image.ocr_status,
            "ocr_markdown": image.ocr_markdown,
            "chunks": image.ocr_chunks,
            "page_status": page_status,
            "issues": review_issues,
        }));
    }

    Ok(Json(serde_json::json!({
        "id": details.id,
        "course_id": details.course_id,
        "session_id": details.session_id,
        "student_id": details.student_id,
        "submitted_at": format_primitive(details.submitted_at),
        "status": details.status,
        "ocr_overall_status": details.ocr_overall_status,
        "llm_precheck_status": details.llm_precheck_status,
        "report_flag": details.report_flag,
        "report_summary": details.report_summary,
        "ocr_error": details.ocr_error,
        "llm_error": details.ai_error,
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
        "ocr_pages": ocr_pages,
        "report_issues": report_issues,
        "scores": scores,
        "student_name": details.student_name,
        "student_username": details.student_username,
        "exam": {
            "id": details.exam_id,
            "course_id": course_id,
            "title": details.exam_title,
            "kind": details.exam_kind
        },
        "tasks": tasks_payload,
    })))
}

pub(super) async fn approve_submission(
    Path((course_id, submission_id)): Path<(String, String)>,
    CurrentUser(teacher): CurrentUser,
    State(state): State<AppState>,
    Json(payload): Json<SubmissionApproveRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &teacher, &course_id, CourseRole::Teacher).await?;

    let submission = repositories::submissions::find_by_id(state.db(), &course_id, &submission_id)
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
        &course_id,
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
    Path((course_id, submission_id)): Path<(String, String)>,
    CurrentUser(teacher): CurrentUser,
    State(state): State<AppState>,
    Json(payload): Json<SubmissionOverrideRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &teacher, &course_id, CourseRole::Teacher).await?;
    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let submission = repositories::submissions::find_by_id(state.db(), &course_id, &submission_id)
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
        &course_id,
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
    Path((course_id, image_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = require_course_membership(&state, &user, &course_id).await?;

    let image = repositories::images::find_by_id(state.db(), &course_id, &image_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch image"))?;

    let Some(image) = image else {
        return Err(ApiError::NotFound("Image not found".to_string()));
    };

    let submission =
        repositories::submissions::fetch_one_by_id(state.db(), &course_id, &image.submission_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let is_owner = submission.student_id == user.id;
    let is_teacher = has_teacher_access(&user, &access.roles);

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
    Path((course_id, submission_id)): Path<(String, String)>,
    CurrentUser(teacher): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &teacher, &course_id, CourseRole::Teacher).await?;

    let submission = repositories::submissions::find_by_id(state.db(), &course_id, &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(submission) = submission else {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    };

    let session =
        repositories::sessions::find_by_id(state.db(), &course_id, &submission.session_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission session"))?
            .ok_or_else(|| {
                ApiError::Internal("Submission session is missing for regrade".to_string())
            })?;
    let exam = repositories::exams::find_by_id(state.db(), &course_id, &session.exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission exam"))?
        .ok_or_else(|| ApiError::Internal("Submission exam is missing for regrade".to_string()))?;

    let processing = WorkProcessingSettings::from_exam_settings_strict(&exam.settings.0)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !processing.ocr_enabled || !processing.llm_precheck_enabled {
        return Err(ApiError::BadRequest(
            "OCR/LLM precheck pipeline is disabled for this work; regrade is not available"
                .to_string(),
        ));
    }

    let queued = repositories::submissions::queue_regrade(
        state.db(),
        &course_id,
        &submission_id,
        super::helpers::now_primitive(),
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update submission"))?;
    if !queued {
        return Err(ApiError::BadRequest(
            "Submission cannot be re-queued in the current state".to_string(),
        ));
    }

    tracing::info!(
        teacher_id = %teacher.id,
        course_id = %course_id,
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
    Path((course_id, submission_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let access = require_course_membership(&state, &user, &course_id).await?;
    let submission = repositories::submissions::find_by_id(state.db(), &course_id, &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?;

    let Some(submission) = submission else {
        return Err(ApiError::NotFound("Submission not found".to_string()));
    };

    let is_owner = submission.student_id == user.id;
    let is_teacher = has_teacher_access(&user, &access.roles);
    if !is_owner && !is_teacher {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (progress, status_message) = match submission.ocr_overall_status {
        OcrOverallStatus::Pending => (10, "OCR в очереди"),
        OcrOverallStatus::Processing => (35, "OCR обрабатывается"),
        OcrOverallStatus::InReview => (60, "Ожидается валидация OCR студентом"),
        OcrOverallStatus::Failed => (65, "OCR не выполнен, требуется ручная проверка"),
        _ => match submission.llm_precheck_status {
            LlmPrecheckStatus::Queued => (75, "LLM-препроверка в очереди"),
            LlmPrecheckStatus::Processing => {
                let mut progress = 85;
                let mut message = "LLM-препроверка выполняется";
                if let Some(started) = submission.ai_request_started_at {
                    let elapsed = OffsetDateTime::now_utc().unix_timestamp()
                        - started.assume_utc().unix_timestamp();
                    if elapsed > 120 {
                        progress = 92;
                        message = "LLM-препроверка: финальная обработка";
                    }
                }
                (progress, message)
            }
            LlmPrecheckStatus::Failed => (95, "LLM-препроверка завершилась ошибкой"),
            LlmPrecheckStatus::Completed | LlmPrecheckStatus::Skipped => match submission.status {
                SubmissionStatus::Preliminary => (100, "Готово к проверке преподавателем"),
                SubmissionStatus::Approved => (100, "Проверено и одобрено"),
                SubmissionStatus::Flagged => (100, "Требует ручной проверки"),
                SubmissionStatus::Rejected => (100, "Отклонено"),
                SubmissionStatus::Processing => (85, "Обработка"),
                SubmissionStatus::Uploaded => (80, "Ожидает обработки"),
            },
        },
    };

    Ok(Json(serde_json::json!({
        "course_id": course_id,
        "submission_id": submission_id,
        "status": submission.status,
        "ocr_overall_status": submission.ocr_overall_status,
        "llm_precheck_status": submission.llm_precheck_status,
        "report_flag": submission.report_flag,
        "report_summary": submission.report_summary,
        "progress": progress,
        "status_message": status_message,
        "ai_score": submission.ai_score,
        "final_score": submission.final_score,
        "max_score": submission.max_score,
        "ai_comments": submission.ai_comments,
        "ai_error": submission.ai_error,
        "ocr_error": submission.ocr_error,
        "ocr_retry_count": submission.ocr_retry_count,
        "ai_retry_count": submission.ai_retry_count,
        "processing_times": {
            "started_at": submission.ai_request_started_at.map(format_primitive),
            "completed_at": submission.ai_request_completed_at.map(format_primitive),
            "duration_seconds": submission.ai_request_duration_seconds
        }
    })))
}

fn has_teacher_access(user: &crate::db::models::User, roles: &[CourseRole]) -> bool {
    user.is_platform_admin || roles.iter().any(|role| *role == CourseRole::Teacher)
}
