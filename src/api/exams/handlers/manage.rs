use axum::{extract::Query, Json};
use validator::Validate;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_membership, require_course_role, CurrentUser};
use crate::core::state::AppState;
use crate::core::time::{primitive_now_utc, to_primitive_utc};
use crate::db::types::{CourseRole, ExamStatus, WorkKind};
use crate::repositories;
use crate::schemas::exam::{ExamResponse, ExamUpdate};
use crate::services::work_processing::WorkProcessingSettings;
use crate::services::work_timing::normalize_duration_for_kind;

use super::super::helpers;
use super::super::queries::DeleteExamQuery;

pub(in crate::api::exams) async fn get_exam(
    axum::extract::Path((course_id, exam_id)): axum::extract::Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
) -> Result<Json<ExamResponse>, ApiError> {
    let access = require_course_membership(&state, &user, &course_id).await?;

    let exam = repositories::exams::find_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    let is_teacher = user.is_platform_admin
        || access.roles.iter().any(|role| matches!(role, CourseRole::Teacher));

    if !is_teacher
        && !matches!(
            exam.status,
            ExamStatus::Published | ExamStatus::Active | ExamStatus::Completed
        )
    {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let task_types = helpers::fetch_task_types(state.db(), &course_id, &exam.id).await?;

    Ok(Json(helpers::exam_to_response(exam, task_types)))
}

pub(in crate::api::exams) async fn update_exam(
    axum::extract::Path((course_id, exam_id)): axum::extract::Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Json(payload): Json<ExamUpdate>,
) -> Result<Json<ExamResponse>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    let exam = repositories::exams::find_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let effective_start = payload.start_time.unwrap_or(exam.start_time.assume_utc());
    let effective_end = payload.end_time.unwrap_or(exam.end_time.assume_utc());
    if effective_end <= effective_start {
        return Err(ApiError::BadRequest("end_time must be after start_time".to_string()));
    }

    let effective_kind = payload.kind.unwrap_or(exam.kind);
    let effective_duration = if matches!(effective_kind, WorkKind::Homework) {
        payload.duration_minutes
    } else {
        payload.duration_minutes.or(exam.duration_minutes)
    };
    let normalized_duration = normalize_duration_for_kind(effective_kind, effective_duration)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let clear_duration = matches!(effective_kind, WorkKind::Homework);
    let duration_minutes = if clear_duration {
        None
    } else if payload.duration_minutes.is_some() || !matches!(exam.kind, WorkKind::Control) {
        normalized_duration
    } else {
        None
    };

    let current_processing = WorkProcessingSettings::from_exam_settings(&exam.settings.0);
    let processing = WorkProcessingSettings {
        ocr_enabled: payload.ocr_enabled.unwrap_or(current_processing.ocr_enabled),
        llm_precheck_enabled: payload
            .llm_precheck_enabled
            .unwrap_or(current_processing.llm_precheck_enabled),
    }
    .validate()
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let base_settings = payload.settings.clone().unwrap_or_else(|| exam.settings.0.clone());
    let merged_settings = processing.merge_into_exam_settings(base_settings);

    let now = primitive_now_utc();
    let start_time = payload.start_time.map(to_primitive_utc);
    let end_time = payload.end_time.map(to_primitive_utc);

    repositories::exams::update(
        state.db(),
        &course_id,
        &exam_id,
        repositories::exams::UpdateExam {
            title: payload.title,
            description: payload.description,
            kind: payload.kind,
            start_time,
            end_time,
            duration_minutes,
            clear_duration,
            settings: Some(merged_settings),
            updated_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update exam"))?;

    let updated = repositories::exams::fetch_one_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch updated exam"))?;

    let task_types = helpers::fetch_task_types(state.db(), &course_id, &updated.id).await?;

    Ok(Json(helpers::exam_to_response(updated, task_types)))
}

pub(in crate::api::exams) async fn delete_exam(
    axum::extract::Path((course_id, exam_id)): axum::extract::Path<(String, String)>,
    Query(params): Query<DeleteExamQuery>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    let exam = repositories::exams::find_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(_exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    let submissions_count = repositories::exams::count_sessions(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to count sessions"))?;

    if submissions_count > 0 && !params.force_delete {
        return Err(ApiError::BadRequest(format!(
            "Cannot delete exam with {submissions_count} existing submission(s). Use force_delete=true to delete anyway."
        )));
    }

    repositories::exams::delete_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to delete exam"))?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub(in crate::api::exams) async fn publish_exam(
    axum::extract::Path((course_id, exam_id)): axum::extract::Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
) -> Result<Json<ExamResponse>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    let exam = repositories::exams::find_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    if exam.status != ExamStatus::Draft {
        return Err(ApiError::BadRequest("Exam is not in draft status".to_string()));
    }

    let task_count = repositories::exams::count_task_types(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to count task types"))?;

    if task_count == 0 {
        return Err(ApiError::BadRequest("Exam must have at least one task type".to_string()));
    }

    let now = primitive_now_utc();
    repositories::exams::publish(state.db(), &course_id, &exam_id, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to publish exam"))?;

    let updated = repositories::exams::fetch_one_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch updated exam"))?;

    let task_types = helpers::fetch_task_types(state.db(), &course_id, &updated.id).await?;

    tracing::info!(
        user_id = %user.id,
        course_id = %course_id,
        exam_id = %updated.id,
        action = "exam_publish",
        "Exam published"
    );

    Ok(Json(helpers::exam_to_response(updated, task_types)))
}
