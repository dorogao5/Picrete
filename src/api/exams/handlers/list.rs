use axum::{extract::Query, Json};

use crate::api::errors::ApiError;
use crate::api::guards::{CurrentTeacher, CurrentUser};
use crate::api::pagination::PaginatedResponse;
use crate::core::state::AppState;
use crate::db::types::UserRole;
use crate::repositories;
use crate::schemas::exam::{format_primitive, ExamSummaryResponse};

use super::super::helpers;
use super::super::queries::{ListExamSubmissionsQuery, ListExamsQuery};

pub(in crate::api::exams) async fn list_exams(
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Query(params): Query<ListExamsQuery>,
) -> Result<Json<PaginatedResponse<ExamSummaryResponse>>, ApiError> {
    let skip = params.skip.max(0);
    let limit = params.limit.clamp(1, 1000);

    let rows = repositories::exams::list_summaries(
        state.db(),
        repositories::exams::ListExamSummariesParams {
            student_visible_only: matches!(user.role, UserRole::Student),
            status: params.status,
            skip,
            limit,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to list exams"))?;

    let total_count = rows.first().map(|row| row.total_count).unwrap_or(0);
    let summaries = rows
        .into_iter()
        .map(|row| ExamSummaryResponse {
            id: row.id,
            title: row.title,
            start_time: format_primitive(row.start_time),
            end_time: format_primitive(row.end_time),
            duration_minutes: row.duration_minutes,
            status: row.status,
            task_count: row.task_count,
            student_count: row.student_count,
            pending_count: row.pending_count,
        })
        .collect();

    Ok(Json(PaginatedResponse { items: summaries, total_count, skip, limit }))
}

pub(in crate::api::exams) async fn list_exam_submissions(
    axum::extract::Path(exam_id): axum::extract::Path<String>,
    Query(params): Query<ListExamSubmissionsQuery>,
    CurrentTeacher(teacher): CurrentTeacher,
    state: axum::extract::State<AppState>,
) -> Result<Json<PaginatedResponse<serde_json::Value>>, ApiError> {
    let exam = repositories::exams::find_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    if !helpers::can_manage_exam(&teacher, &exam) {
        return Err(ApiError::Forbidden("You can only view submissions for your own exams"));
    }

    let rows = repositories::exams::list_submissions_by_exam(
        state.db(),
        &exam_id,
        params.status,
        params.skip,
        params.limit,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to list submissions"))?;
    let total_count =
        repositories::exams::count_submissions_by_exam(state.db(), &exam_id, params.status)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to count submissions"))?;

    let mut response = Vec::new();
    for row in rows {
        response.push(serde_json::json!({
            "id": row.id,
            "student_id": row.student_id,
            "student_isu": row.student_isu,
            "student_name": row.student_name,
            "submitted_at": format_primitive(row.submitted_at),
            "status": row.status,
            "ai_score": row.ai_score,
            "final_score": row.final_score,
            "max_score": row.max_score,
        }));
    }

    Ok(Json(PaginatedResponse {
        items: response,
        total_count,
        skip: params.skip.max(0),
        limit: params.limit.clamp(1, 1000),
    }))
}
