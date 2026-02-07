use axum::{extract::Query, Json};
use time::PrimitiveDateTime;
use uuid::Uuid;

use validator::Validate;

use crate::api::errors::ApiError;
use crate::api::guards::{CurrentTeacher, CurrentUser};
use crate::api::pagination::PaginatedResponse;
use crate::core::state::AppState;
use crate::core::time::{primitive_now_utc, to_primitive_utc};
use crate::db::models::Exam;
use crate::db::types::{ExamStatus, SubmissionStatus, UserRole};
use crate::repositories;
use crate::schemas::exam::{
    format_primitive, ExamCreate, ExamResponse, ExamSummaryResponse, ExamUpdate, TaskTypeCreate,
};
use sqlx::{Postgres, QueryBuilder, Row};

use super::helpers;
use super::queries::{DeleteExamQuery, ListExamSubmissionsQuery, ListExamsQuery};

pub(super) async fn create_exam(
    CurrentTeacher(teacher): CurrentTeacher,
    state: axum::extract::State<AppState>,
    Json(payload): Json<ExamCreate>,
) -> Result<(axum::http::StatusCode, Json<ExamResponse>), ApiError> {
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

    let exam = sqlx::query_as::<_, Exam>(&format!(
        "INSERT INTO exams (
            id, title, description, start_time, end_time, duration_minutes, timezone,
            max_attempts, allow_breaks, break_duration_minutes, auto_save_interval,
            status, created_by, created_at, updated_at, settings
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
        RETURNING {}",
        repositories::exams::COLUMNS,
    ))
    .bind(Uuid::new_v4().to_string())
    .bind(payload.title)
    .bind(payload.description)
    .bind(start_time)
    .bind(end_time)
    .bind(payload.duration_minutes)
    .bind(payload.timezone)
    .bind(payload.max_attempts)
    .bind(payload.allow_breaks)
    .bind(payload.break_duration_minutes)
    .bind(payload.auto_save_interval)
    .bind(ExamStatus::Draft)
    .bind(teacher.id)
    .bind(now)
    .bind(now)
    .bind(payload.settings)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create exam"))?;

    let task_types = helpers::insert_task_types(&mut tx, &exam.id, payload.task_types).await?;
    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    Ok((axum::http::StatusCode::CREATED, Json(helpers::exam_to_response(exam, task_types))))
}

pub(super) async fn list_exams(
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Query(params): Query<ListExamsQuery>,
) -> Result<Json<PaginatedResponse<ExamSummaryResponse>>, ApiError> {
    let skip = params.skip.max(0);
    let limit = params.limit.clamp(1, 1000);

    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT e.id, e.title, e.start_time, e.end_time, e.duration_minutes, e.status,
                COALESCE(tc.cnt, 0) AS task_count,
                COALESCE(sc.cnt, 0) AS student_count,
                COALESCE(pc.cnt, 0) AS pending_count,
                COUNT(*) OVER() AS total_count
         FROM exams e
         LEFT JOIN (SELECT exam_id, COUNT(*) AS cnt FROM task_types GROUP BY exam_id) tc
             ON tc.exam_id = e.id
         LEFT JOIN (SELECT exam_id, COUNT(DISTINCT student_id) AS cnt FROM exam_sessions GROUP BY exam_id) sc
             ON sc.exam_id = e.id
         LEFT JOIN (
             SELECT es.exam_id, COUNT(*) AS cnt
             FROM submissions s
             JOIN exam_sessions es ON s.session_id = es.id
             WHERE s.status = ",
    );
    builder.push_bind(SubmissionStatus::Preliminary);
    builder.push(" GROUP BY es.exam_id) pc ON pc.exam_id = e.id");

    let has_student_filter = matches!(user.role, UserRole::Student);

    if has_student_filter {
        builder.push(" WHERE e.status IN (");
        builder.push_bind(ExamStatus::Published);
        builder.push(", ");
        builder.push_bind(ExamStatus::Active);
        builder.push(", ");
        builder.push_bind(ExamStatus::Completed);
        builder.push(")");
    }

    if let Some(status) = params.status {
        if !has_student_filter {
            builder.push(" WHERE ");
        } else {
            builder.push(" AND ");
        }
        builder.push("e.status = ");
        builder.push_bind(status);
    }

    builder.push(" ORDER BY e.start_time DESC");
    builder.push(" OFFSET ");
    builder.push_bind(skip);
    builder.push(" LIMIT ");
    builder.push_bind(limit);

    let rows = builder
        .build()
        .fetch_all(state.db())
        .await
        .map_err(|e| ApiError::internal(e, "Failed to list exams"))?;

    let mut summaries = Vec::new();
    let mut total_count = 0;

    for row in rows {
        let exam_id: String = row.try_get("id").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let title: String = row.try_get("title").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let start_time: PrimitiveDateTime =
            row.try_get("start_time").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let end_time: PrimitiveDateTime =
            row.try_get("end_time").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let duration_minutes: i32 =
            row.try_get("duration_minutes").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let status: ExamStatus =
            row.try_get("status").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let task_count: i64 =
            row.try_get("task_count").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let student_count: i64 =
            row.try_get("student_count").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let pending_count: i64 =
            row.try_get("pending_count").map_err(|e| ApiError::internal(e, "Bad row"))?;
        total_count = row.try_get("total_count").map_err(|e| ApiError::internal(e, "Bad row"))?;

        summaries.push(ExamSummaryResponse {
            id: exam_id,
            title,
            start_time: format_primitive(start_time),
            end_time: format_primitive(end_time),
            duration_minutes,
            status,
            task_count,
            student_count,
            pending_count,
        });
    }

    Ok(Json(PaginatedResponse { items: summaries, total_count, skip, limit }))
}

pub(super) async fn get_exam(
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
        && !matches!(exam.status, ExamStatus::Published | ExamStatus::Active | ExamStatus::Completed)
    {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let task_types = helpers::fetch_task_types(state.db(), &exam.id).await?;

    Ok(Json(helpers::exam_to_response(exam, task_types)))
}

pub(super) async fn update_exam(
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

    sqlx::query(
        "UPDATE exams SET
            title = COALESCE($1, title),
            description = COALESCE($2, description),
            start_time = COALESCE($3, start_time),
            end_time = COALESCE($4, end_time),
            duration_minutes = COALESCE($5, duration_minutes),
            settings = COALESCE($6::jsonb, settings),
            updated_at = $7
         WHERE id = $8",
    )
    .bind(payload.title)
    .bind(payload.description)
    .bind(start_time)
    .bind(end_time)
    .bind(payload.duration_minutes)
    .bind(payload.settings)
    .bind(now)
    .bind(&exam_id)
    .execute(state.db())
    .await
    .map_err(|e| ApiError::internal(e, "Failed to update exam"))?;

    let updated = repositories::exams::fetch_one_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch updated exam"))?;

    let task_types = helpers::fetch_task_types(state.db(), &updated.id).await?;

    Ok(Json(helpers::exam_to_response(updated, task_types)))
}

pub(super) async fn delete_exam(
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

pub(super) async fn publish_exam(
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

pub(super) async fn add_task_type(
    axum::extract::Path(exam_id): axum::extract::Path<String>,
    CurrentTeacher(teacher): CurrentTeacher,
    state: axum::extract::State<AppState>,
    Json(payload): Json<TaskTypeCreate>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let exam = repositories::exams::find_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    let Some(exam) = exam else {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    };

    if !helpers::can_manage_exam(&teacher, &exam) {
        return Err(ApiError::Forbidden("You can only add task types to your own exams"));
    }

    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start transaction"))?;

    let now = primitive_now_utc();
    let task_type_id = Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO task_types (
            id, exam_id, title, description, order_index, max_score, rubric,
            difficulty, taxonomy_tags, formulas, units, validation_rules,
            created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
    )
    .bind(&task_type_id)
    .bind(&exam_id)
    .bind(payload.title)
    .bind(payload.description)
    .bind(payload.order_index)
    .bind(payload.max_score)
    .bind(payload.rubric)
    .bind(payload.difficulty)
    .bind(payload.taxonomy_tags)
    .bind(payload.formulas)
    .bind(payload.units)
    .bind(payload.validation_rules)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create task type"))?;

    helpers::insert_variants(&mut tx, &task_type_id, payload.variants).await?;
    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({
            "message": "Task type added successfully",
            "task_type_id": task_type_id
        })),
    ))
}

pub(super) async fn list_exam_submissions(
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
