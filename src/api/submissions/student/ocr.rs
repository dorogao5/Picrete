use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    Json,
};

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_role, CurrentUser};
use crate::core::state::AppState;
use crate::db::types::{CourseRole, OcrImageStatus, OcrOverallStatus, OcrPageStatus};
use crate::repositories;
use crate::repositories::ocr_reviews::NewOcrIssue;
use crate::schemas::submission::{
    FinalizeOcrReviewRequest, OcrFinalizeAction, OcrIssueResponse, OcrPageResponse,
    OcrPagesResponse, OcrReviewUpsertRequest, SubmissionNextStep, SubmissionResponse,
};
use crate::services::work_processing::WorkProcessingSettings;

pub(in crate::api::submissions) async fn get_ocr_pages(
    Path((course_id, session_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<OcrPagesResponse>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let exam =
        crate::api::submissions::helpers::fetch_exam(state.db(), &course_id, &session.exam_id)
            .await?;
    let processing = WorkProcessingSettings::from_exam_settings_strict(&exam.settings.0)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !processing.ocr_enabled {
        return Err(ApiError::NotFound("OCR_NOT_ENABLED_FOR_WORK".to_string()));
    }

    let submission =
        repositories::submissions::find_by_session(state.db(), &course_id, &session_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?
            .ok_or_else(|| {
                ApiError::BadRequest("No submission found for this session".to_string())
            })?;

    let images = repositories::images::list_by_submission(state.db(), &course_id, &submission.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch images"))?;
    let reviews = repositories::ocr_reviews::list_reviews_by_submission(
        state.db(),
        &course_id,
        &submission.id,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to fetch OCR reviews"))?;
    let issues = repositories::ocr_reviews::list_issues_by_submission(
        state.db(),
        &course_id,
        &submission.id,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to fetch OCR issues"))?;

    let review_by_image: HashMap<String, crate::db::models::SubmissionOcrReview> =
        reviews.into_iter().map(|review| (review.image_id.clone(), review)).collect();

    let mut issues_by_review: HashMap<String, Vec<OcrIssueResponse>> = HashMap::new();
    for issue in issues {
        issues_by_review.entry(issue.ocr_review_id.clone()).or_default().push(OcrIssueResponse {
            id: issue.id,
            review_id: issue.ocr_review_id,
            image_id: issue.image_id,
            anchor: issue.anchor.0,
            original_text: issue.original_text,
            suggested_text: issue.suggested_text,
            note: issue.note,
            severity: issue.severity,
            created_at: crate::schemas::submission::format_primitive(issue.created_at),
        });
    }

    let storage = state.storage().ok_or_else(|| {
        ApiError::ServiceUnavailable("S3 storage is not configured for OCR review".to_string())
    })?;
    let mut pages = Vec::with_capacity(images.len());
    for image in images {
        if !image.file_path.starts_with("submissions/") {
            return Err(ApiError::BadRequest(
                "OCR review image is stored in unsupported local storage path".to_string(),
            ));
        }
        let image_view_url = Some(
            storage
                .presign_get(&image.file_path, std::time::Duration::from_secs(300))
                .await
                .map_err(|e| ApiError::internal(e, "Failed to generate OCR review image URL"))?,
        );

        let (page_status, review_issues) = if let Some(review) = review_by_image.get(&image.id) {
            (Some(review.page_status), issues_by_review.remove(&review.id).unwrap_or_default())
        } else {
            (None, Vec::new())
        };

        pages.push(OcrPageResponse {
            image_id: image.id,
            image_view_url,
            ocr_status: image.ocr_status,
            ocr_markdown: image.ocr_markdown,
            chunks: image.ocr_chunks.map(|value| value.0),
            page_status,
            issues: review_issues,
        });
    }

    Ok(Json(OcrPagesResponse {
        submission_id: submission.id,
        ocr_status: submission.ocr_overall_status,
        llm_precheck_status: submission.llm_precheck_status,
        report_flag: submission.report_flag,
        report_summary: submission.report_summary,
        pages,
    }))
}

pub(in crate::api::submissions) async fn review_ocr_page(
    Path((course_id, session_id, image_id)): Path<(String, String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(payload): Json<OcrReviewUpsertRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let exam =
        crate::api::submissions::helpers::fetch_exam(state.db(), &course_id, &session.exam_id)
            .await?;
    let processing = WorkProcessingSettings::from_exam_settings_strict(&exam.settings.0)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !processing.ocr_enabled {
        return Err(ApiError::NotFound("OCR_NOT_ENABLED_FOR_WORK".to_string()));
    }

    if payload.page_status == OcrPageStatus::Approved && !payload.issues.is_empty() {
        return Err(ApiError::BadRequest("Approved page cannot contain OCR issues".to_string()));
    }
    if payload.page_status == OcrPageStatus::Reported && payload.issues.is_empty() {
        return Err(ApiError::BadRequest(
            "Reported page must contain at least one OCR issue".to_string(),
        ));
    }

    for issue in &payload.issues {
        if issue.note.trim().is_empty() {
            return Err(ApiError::BadRequest("OCR issue note cannot be empty".to_string()));
        }
        validate_issue_anchor(&issue.anchor)?;
    }

    let submission =
        repositories::submissions::find_by_session(state.db(), &course_id, &session_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?
            .ok_or_else(|| {
                ApiError::BadRequest("No submission found for this session".to_string())
            })?;

    if submission.ocr_overall_status != OcrOverallStatus::InReview {
        return Err(ApiError::BadRequest(format!(
            "OCR page review is not available in status {:?}",
            submission.ocr_overall_status
        )));
    }

    let image = repositories::images::find_by_id(state.db(), &course_id, &image_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch image"))?
        .ok_or_else(|| ApiError::NotFound("Image not found".to_string()))?;

    if image.submission_id != submission.id {
        return Err(ApiError::Forbidden("Image does not belong to current submission"));
    }
    if image.ocr_status != OcrImageStatus::Ready {
        return Err(ApiError::BadRequest("OCR page is not ready yet".to_string()));
    }

    let now = crate::api::submissions::helpers::now_primitive();
    let issues = payload
        .issues
        .iter()
        .map(|issue| NewOcrIssue {
            anchor: issue.anchor.clone(),
            original_text: issue.original_text.clone(),
            suggested_text: issue.suggested_text.clone(),
            note: issue.note.clone(),
            severity: issue.severity,
        })
        .collect::<Vec<_>>();

    repositories::ocr_reviews::upsert_page_review(
        state.db(),
        &course_id,
        &submission.id,
        &image_id,
        &user.id,
        payload.page_status,
        &issues,
        now,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to save OCR page review"))?;

    let stats = repositories::ocr_reviews::review_completion_stats(
        state.db(),
        &course_id,
        &submission.id,
        &user.id,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to fetch OCR review stats"))?;

    Ok(Json(serde_json::json!({
        "message": "OCR review saved",
        "completion": {
            "total_pages": stats.total_pages,
            "reviewed_pages": stats.reviewed_pages,
            "reported_pages": stats.reported_pages,
            "total_issues": stats.total_issues,
        }
    })))
}

pub(in crate::api::submissions) async fn finalize_ocr_review(
    Path((course_id, session_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(payload): Json<FinalizeOcrReviewRequest>,
) -> Result<Json<SubmissionResponse>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let exam =
        crate::api::submissions::helpers::fetch_exam(state.db(), &course_id, &session.exam_id)
            .await?;
    let processing = WorkProcessingSettings::from_exam_settings_strict(&exam.settings.0)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if !processing.ocr_enabled {
        return Err(ApiError::NotFound("OCR_NOT_ENABLED_FOR_WORK".to_string()));
    }

    let submission =
        repositories::submissions::find_by_session(state.db(), &course_id, &session_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submission"))?
            .ok_or_else(|| {
                ApiError::BadRequest("No submission found for this session".to_string())
            })?;

    if submission.ocr_overall_status != OcrOverallStatus::InReview {
        return Err(ApiError::BadRequest(format!(
            "OCR finalize is not available in status {:?}",
            submission.ocr_overall_status
        )));
    }

    let stats = repositories::ocr_reviews::review_completion_stats(
        state.db(),
        &course_id,
        &submission.id,
        &user.id,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to fetch OCR review stats"))?;

    if stats.total_pages == 0 {
        return Err(ApiError::BadRequest(
            "Cannot finalize OCR review without uploaded pages".to_string(),
        ));
    }
    if stats.reviewed_pages < stats.total_pages {
        return Err(ApiError::BadRequest("OCR_REVIEW_INCOMPLETE".to_string()));
    }

    let has_issues = stats.total_issues > 0;
    if payload.action == OcrFinalizeAction::Submit && has_issues {
        return Err(ApiError::BadRequest(
            "Found OCR issues. Use REPORT action instead of SUBMIT".to_string(),
        ));
    }
    if payload.action == OcrFinalizeAction::Report && !has_issues {
        return Err(ApiError::BadRequest("REPORT requires at least one OCR issue".to_string()));
    }
    if payload.action == OcrFinalizeAction::Report
        && payload.report_summary.as_ref().map(|summary| summary.trim().is_empty()).unwrap_or(true)
    {
        return Err(ApiError::BadRequest("REPORT requires non-empty report_summary".to_string()));
    }

    let now = crate::api::submissions::helpers::now_primitive();
    let (ocr_status, report_flag) = match payload.action {
        OcrFinalizeAction::Submit => (OcrOverallStatus::Validated, false),
        OcrFinalizeAction::Report => (OcrOverallStatus::Reported, true),
    };

    if processing.llm_precheck_enabled {
        repositories::submissions::queue_llm_precheck_after_ocr(
            state.db(),
            &course_id,
            &submission.id,
            ocr_status,
            report_flag,
            payload.report_summary.clone(),
            now,
        )
        .await
        .map_err(|e| ApiError::internal(e, "Failed to queue LLM precheck"))?;
    } else {
        repositories::submissions::skip_llm_precheck_after_ocr(
            state.db(),
            &course_id,
            &submission.id,
            ocr_status,
            report_flag,
            payload.report_summary.clone(),
            now,
        )
        .await
        .map_err(|e| ApiError::internal(e, "Failed to finalize OCR review"))?;
    }

    let submission = repositories::submissions::find_by_id(state.db(), &course_id, &submission.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch updated submission"))?
        .ok_or_else(|| ApiError::Internal("Submission missing after OCR finalize".to_string()))?;
    let images =
        crate::api::submissions::helpers::fetch_images(state.db(), &course_id, &submission.id)
            .await?;
    let scores =
        crate::api::submissions::helpers::fetch_scores(state.db(), &course_id, &submission.id)
            .await?;
    let response =
        crate::api::submissions::helpers::to_submission_response(submission, images, scores);

    Ok(Json(crate::api::submissions::helpers::with_next_step(response, SubmissionNextStep::Result)))
}

fn validate_issue_anchor(anchor: &serde_json::Value) -> Result<(), ApiError> {
    let Some(anchor) = anchor.as_object() else {
        return Err(ApiError::BadRequest("OCR issue anchor must be a JSON object".to_string()));
    };

    let page = anchor.get("page").and_then(serde_json::Value::as_i64);
    if page.is_none() || page.unwrap_or(-1) < 0 {
        return Err(ApiError::BadRequest(
            "OCR issue anchor must include non-negative 'page'".to_string(),
        ));
    }

    let block_type = anchor
        .get("block_type")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if block_type.is_empty() {
        return Err(ApiError::BadRequest(
            "OCR issue anchor must include non-empty 'block_type'".to_string(),
        ));
    }

    if !has_valid_bbox(anchor) && !has_valid_polygon(anchor) {
        return Err(ApiError::BadRequest(
            "OCR issue anchor must include valid 'bbox' or 'polygon' geometry".to_string(),
        ));
    }

    Ok(())
}

fn has_valid_bbox(anchor: &serde_json::Map<String, serde_json::Value>) -> bool {
    let Some(bbox) = anchor.get("bbox").and_then(serde_json::Value::as_array) else {
        return false;
    };
    bbox.len() >= 4 && bbox.iter().all(|item| item.as_f64().is_some())
}

fn has_valid_polygon(anchor: &serde_json::Map<String, serde_json::Value>) -> bool {
    let Some(polygon) = anchor.get("polygon").and_then(serde_json::Value::as_array) else {
        return false;
    };
    if polygon.len() < 4 {
        return false;
    }

    polygon.iter().all(|point| {
        point.as_array().is_some_and(|coords| {
            coords.len() >= 2 && coords.iter().all(|item| item.as_f64().is_some())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::validate_issue_anchor;

    #[test]
    fn validate_issue_anchor_accepts_bbox_with_page_and_block_type() {
        let anchor = serde_json::json!({
            "page": 1,
            "block_type": "text",
            "bbox": [10.0, 20.0, 30.0, 40.0]
        });
        assert!(validate_issue_anchor(&anchor).is_ok());
    }

    #[test]
    fn validate_issue_anchor_rejects_missing_geometry() {
        let anchor = serde_json::json!({
            "page": 1,
            "block_type": "text"
        });
        assert!(validate_issue_anchor(&anchor).is_err());
    }
}
