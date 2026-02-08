use axum::{extract::Query, Json};
use validator::Validate;

use crate::api::errors::ApiError;
use crate::api::guards::{CurrentTeacher, CurrentUser};
use crate::core::state::AppState;
use crate::core::time::{primitive_now_utc, to_primitive_utc};
use crate::db::types::{ExamStatus, UserRole};
use crate::repositories;
use crate::schemas::exam::{ExamResponse, ExamUpdate};

use super::super::helpers;
use super::super::queries::DeleteExamQuery;

pub(in crate::api::exams) async fn get_exam(
    axum::extract::Path(exam_id): axum::extract::Path<String>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
) -> Result<Json<ExamResponse>, ApiError> {
    let exam = repositories::exams::find_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    if matches!(user.role, UserRole::Student)
        && !matches!(
            exam.status,
            ExamStatus::Published | ExamStatus::Active | ExamStatus::Completed
        )
    {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let task_types = helpers::fetch_task_types(state.db(), &exam.id).await?;

    Ok(Json(helpers::exam_to_response(exam, task_types)))
}

pub(in crate::api::exams) async fn update_exam(
    axum::extract::Path(exam_id): axum::extract::Path<String>,
    CurrentTeacher(teacher): CurrentTeacher,
    state: axum::extract::State<AppState>,
    Json(payload): Json<ExamUpdate>,
) -> Result<Json<ExamResponse>, ApiError> {
    let exam = repositories::exams::find_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    if !helpers::can_manage_exam(&teacher, &exam) {
        return Err(ApiError::Forbidden("You can only update your own exams"));
    }

    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let effective_start = payload.start_time.unwrap_or(exam.start_time.assume_utc());
    let effective_end = payload.end_time.unwrap_or(exam.end_time.assume_utc());
    if effective_end <= effective_start {
        return Err(ApiError::BadRequest("end_time must be after start_time".to_string()));
    }

    let now = primitive_now_utc();
    let start_time = payload.start_time.map(to_primitive_utc);
    let end_time = payload.end_time.map(to_primitive_utc);

    repositories::exams::update(
        state.db(),
        &exam_id,
        repositories::exams::UpdateExam {
            title: payload.title,
            description: payload.description,
            start_time,
            end_time,
            duration_minutes: payload.duration_minutes,
            settings: payload.settings,
            updated_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update exam"))?;

    let updated = repositories::exams::fetch_one_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch updated exam"))?;

    let task_types = helpers::fetch_task_types(state.db(), &updated.id).await?;

    Ok(Json(helpers::exam_to_response(updated, task_types)))
}

pub(in crate::api::exams) async fn delete_exam(
    axum::extract::Path(exam_id): axum::extract::Path<String>,
    Query(params): Query<DeleteExamQuery>,
    CurrentTeacher(teacher): CurrentTeacher,
    state: axum::extract::State<AppState>,
) -> Result<axum::http::StatusCode, ApiError> {
    let exam = repositories::exams::find_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    if !helpers::can_manage_exam(&teacher, &exam) {
        return Err(ApiError::Forbidden("You can only delete your own exams"));
    }

    let submissions_count = repositories::exams::count_sessions(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to count sessions"))?;

    if submissions_count > 0 && !params.force_delete {
        return Err(ApiError::BadRequest(format!(
            "Cannot delete exam with {submissions_count} existing submission(s). Use force_delete=true to delete anyway."
        )));
    }

    repositories::exams::delete_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to delete exam"))?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub(in crate::api::exams) async fn publish_exam(
    axum::extract::Path(exam_id): axum::extract::Path<String>,
    CurrentTeacher(teacher): CurrentTeacher,
    state: axum::extract::State<AppState>,
) -> Result<Json<ExamResponse>, ApiError> {
    let exam = repositories::exams::find_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    if !helpers::can_manage_exam(&teacher, &exam) {
        return Err(ApiError::Forbidden("You can only publish your own exams"));
    }

    if exam.status != ExamStatus::Draft {
        return Err(ApiError::BadRequest("Exam is not in draft status".to_string()));
    }

    let task_count = repositories::exams::count_task_types(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to count task types"))?;

    if task_count == 0 {
        return Err(ApiError::BadRequest("Exam must have at least one task type".to_string()));
    }

    let now = primitive_now_utc();
    repositories::exams::publish(state.db(), &exam_id, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to publish exam"))?;

    let updated = repositories::exams::fetch_one_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch updated exam"))?;

    let task_types = helpers::fetch_task_types(state.db(), &updated.id).await?;

    tracing::info!(
        teacher_id = %teacher.id,
        exam_id = %updated.id,
        action = "exam_publish",
        "Exam published"
    );

    Ok(Json(helpers::exam_to_response(updated, task_types)))
}
