use axum::{
    extract::{Query, State},
    Json,
};
use std::collections::HashMap;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_role, CurrentUser};
use crate::api::pagination::PaginatedResponse;
use crate::core::state::AppState;
use crate::db::types::CourseRole;
use crate::repositories;
use crate::schemas::submission::{
    format_primitive, SubmissionImageResponse, SubmissionScoreResponse,
};

pub(in crate::api::submissions) async fn get_my_submissions(
    axum::extract::Path(course_id): axum::extract::Path<String>,
    Query(params): Query<crate::api::submissions::ListSubmissionsQuery>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<PaginatedResponse<serde_json::Value>>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let skip = params.skip.max(0);
    let limit = params.limit.clamp(1, 1000);
    let sessions = repositories::sessions::list_by_student(
        state.db(),
        &course_id,
        &user.id,
        params.status,
        skip,
        limit,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to fetch sessions"))?;
    let total_count =
        repositories::sessions::count_by_student(state.db(), &course_id, &user.id, params.status)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to count sessions"))?;

    if sessions.is_empty() {
        return Ok(Json(PaginatedResponse { items: vec![], total_count, skip, limit }));
    }

    let session_ids = sessions.iter().map(|session| session.id.clone()).collect::<Vec<_>>();
    let submissions =
        repositories::submissions::list_by_sessions(state.db(), &course_id, &session_ids)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch submissions"))?;
    let mut submissions_by_session = submissions
        .into_iter()
        .map(|submission| (submission.session_id.clone(), submission))
        .collect::<HashMap<_, _>>();

    let exam_ids = sessions.iter().map(|session| session.exam_id.clone()).collect::<Vec<_>>();
    let exam_titles = repositories::exams::list_titles_by_ids(state.db(), &course_id, &exam_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam titles"))?
        .into_iter()
        .collect::<HashMap<_, _>>();

    let submission_ids =
        submissions_by_session.values().map(|submission| submission.id.clone()).collect::<Vec<_>>();

    let images = repositories::images::list_by_submissions(state.db(), &course_id, &submission_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission images"))?;
    let mut images_by_submission: HashMap<String, Vec<SubmissionImageResponse>> = HashMap::new();
    for image in images {
        images_by_submission.entry(image.submission_id.clone()).or_default().push(
            SubmissionImageResponse {
                id: image.id,
                course_id: image.course_id,
                filename: image.filename,
                order_index: image.order_index,
                file_size: image.file_size,
                mime_type: image.mime_type,
                is_processed: image.is_processed,
                ocr_status: image.ocr_status,
                ocr_text: image.ocr_text,
                ocr_markdown: image.ocr_markdown,
                ocr_chunks: None,
                quality_score: image.quality_score,
                uploaded_at: format_primitive(image.uploaded_at),
            },
        );
    }

    let scores = repositories::scores::list_by_submissions(state.db(), &course_id, &submission_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch submission scores"))?;
    let mut scores_by_submission: HashMap<String, Vec<SubmissionScoreResponse>> = HashMap::new();
    for score in scores {
        scores_by_submission.entry(score.submission_id.clone()).or_default().push(
            SubmissionScoreResponse {
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
            },
        );
    }

    let mut response = Vec::new();
    for session in sessions {
        let submission = submissions_by_session.remove(&session.id);
        let submission_id = submission.as_ref().map(|sub| sub.id.clone());
        let images = submission_id
            .as_ref()
            .and_then(|id| images_by_submission.remove(id))
            .unwrap_or_default();
        let scores = submission_id
            .as_ref()
            .and_then(|id| scores_by_submission.remove(id))
            .unwrap_or_default();

        response.push(serde_json::json!({
            "id": submission.as_ref().map(|s| &s.id),
            "course_id": course_id,
            "session_id": session.id,
            "exam_id": session.exam_id,
            "exam_title": exam_titles.get(&session.exam_id).cloned().unwrap_or_else(|| "Unknown".to_string()),
            "submitted_at": submission.as_ref().map(|s| format_primitive(s.submitted_at)),
            "status": submission.as_ref().map(|s| &s.status),
            "ocr_overall_status": submission.as_ref().map(|s| &s.ocr_overall_status),
            "llm_precheck_status": submission.as_ref().map(|s| &s.llm_precheck_status),
            "report_flag": submission.as_ref().map(|s| s.report_flag).unwrap_or(false),
            "ai_score": submission.as_ref().and_then(|s| s.ai_score),
            "final_score": submission.as_ref().and_then(|s| s.final_score),
            "max_score": submission.as_ref().map(|s| s.max_score),
            "images": images,
            "scores": scores,
            "teacher_comments": submission.as_ref().and_then(|s| s.teacher_comments.clone()),
        }));
    }

    Ok(Json(PaginatedResponse { items: response, total_count, skip, limit }))
}
