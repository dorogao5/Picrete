use axum::{extract::Query, Json};

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_membership, require_course_role, CurrentUser};
use crate::api::pagination::PaginatedResponse;
use crate::core::state::AppState;
use crate::db::types::CourseRole;
use crate::repositories;
use crate::schemas::exam::{format_primitive, ExamSummaryResponse};

use super::super::queries::{ListExamSubmissionsQuery, ListExamsQuery};

pub(in crate::api::exams) async fn list_exams(
    axum::extract::Path(course_id): axum::extract::Path<String>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Query(params): Query<ListExamsQuery>,
) -> Result<Json<PaginatedResponse<ExamSummaryResponse>>, ApiError> {
    let access = require_course_membership(&state, &user, &course_id).await?;

    let skip = params.skip.max(0);
    let limit = params.limit.clamp(1, 1000);
    let is_teacher = user.is_platform_admin
        || access.roles.iter().any(|role| matches!(role, CourseRole::Teacher));

    let rows = repositories::exams::list_summaries(
        state.db(),
        repositories::exams::ListExamSummariesParams {
            course_id: course_id.clone(),
            student_visible_only: !is_teacher,
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
            course_id: row.course_id,
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
    axum::extract::Path((course_id, exam_id)): axum::extract::Path<(String, String)>,
    Query(params): Query<ListExamSubmissionsQuery>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
) -> Result<Json<PaginatedResponse<serde_json::Value>>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    let exam = repositories::exams::find_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(_exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    let rows = repositories::exams::list_submissions_by_exam(
        state.db(),
        &course_id,
        &exam_id,
        params.status,
        params.skip,
        params.limit,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to list submissions"))?;
    let total_count = repositories::exams::count_submissions_by_exam(
        state.db(),
        &course_id,
        &exam_id,
        params.status,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to count submissions"))?;

    let mut response = Vec::new();
    for row in rows {
        response.push(serde_json::json!({
            "id": row.id,
            "student_id": row.student_id,
            "student_username": row.student_username,
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
