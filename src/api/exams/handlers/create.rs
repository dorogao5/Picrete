use axum::Json;
use uuid::Uuid;
use validator::Validate;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_role, CurrentUser};
use crate::core::state::AppState;
use crate::core::time::{primitive_now_utc, to_primitive_utc};
use crate::db::types::{CourseRole, ExamStatus};
use crate::repositories;
use crate::schemas::exam::{ExamCreate, ExamResponse, TaskTypeCreate};

use super::super::helpers;

pub(in crate::api::exams) async fn create_exam(
    axum::extract::Path(course_id): axum::extract::Path<String>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Json(payload): Json<ExamCreate>,
) -> Result<(axum::http::StatusCode, Json<ExamResponse>), ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    if payload.end_time <= payload.start_time {
        return Err(ApiError::BadRequest("end_time must be after start_time".to_string()));
    }

    let start_time = to_primitive_utc(payload.start_time);
    let end_time = to_primitive_utc(payload.end_time);

    let now = primitive_now_utc();
    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start transaction"))?;

    let exam_id = Uuid::new_v4().to_string();
    let exam = repositories::exams::create(
        &mut *tx,
        repositories::exams::CreateExam {
            id: &exam_id,
            course_id: &course_id,
            title: &payload.title,
            description: payload.description.as_deref(),
            start_time,
            end_time,
            duration_minutes: payload.duration_minutes,
            timezone: &payload.timezone,
            max_attempts: payload.max_attempts,
            allow_breaks: payload.allow_breaks,
            break_duration_minutes: payload.break_duration_minutes,
            auto_save_interval: payload.auto_save_interval,
            status: ExamStatus::Draft,
            created_by: &user.id,
            created_at: now,
            updated_at: now,
            settings: payload.settings.clone(),
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create exam"))?;

    let task_types =
        helpers::insert_task_types(&mut tx, &course_id, &exam.id, payload.task_types).await?;
    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    Ok((axum::http::StatusCode::CREATED, Json(helpers::exam_to_response(exam, task_types))))
}

pub(in crate::api::exams) async fn add_task_type(
    axum::extract::Path((course_id, exam_id)): axum::extract::Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Json(payload): Json<TaskTypeCreate>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Teacher).await?;

    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let exam = repositories::exams::find_by_id(state.db(), &course_id, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(_exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start transaction"))?;

    let now = primitive_now_utc();
    let task_type_id = Uuid::new_v4().to_string();

    let TaskTypeCreate {
        title,
        description,
        order_index,
        max_score,
        rubric,
        difficulty,
        taxonomy_tags,
        formulas,
        units,
        validation_rules,
        variants,
    } = payload;

    repositories::task_types::create(
        &mut *tx,
        repositories::task_types::CreateTaskType {
            id: &task_type_id,
            course_id: &course_id,
            exam_id: &exam_id,
            title: &title,
            description: &description,
            order_index,
            max_score,
            rubric,
            difficulty,
            taxonomy_tags,
            formulas,
            units,
            validation_rules,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create task type"))?;

    helpers::insert_variants(&mut tx, &course_id, &task_type_id, variants).await?;
    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({
            "message": "Task type added successfully",
            "task_type_id": task_type_id
        })),
    ))
}
