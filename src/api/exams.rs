use axum::{extract::Query, routing::get, routing::post, Json, Router};
use serde::Deserialize;
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};
use uuid::Uuid;

use validator::Validate;

use crate::api::errors::ApiError;
use crate::api::guards::{CurrentTeacher, CurrentUser};
use crate::core::state::AppState;
use crate::db::models::Exam;
use crate::db::types::{ExamStatus, SubmissionStatus, UserRole};
use crate::repositories;
use crate::schemas::exam::{
    format_primitive, ExamCreate, ExamResponse, ExamSummaryResponse, ExamUpdate, TaskTypeCreate,
    TaskTypeResponse, TaskVariantCreate, TaskVariantResponse,
};
use sqlx::{types::Json as SqlxJson, Postgres, QueryBuilder, Row};

#[derive(Debug, Deserialize)]
pub(crate) struct ListExamsQuery {
    #[serde(default)]
    skip: i64,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    status: Option<ExamStatus>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeleteExamQuery {
    #[serde(default)]
    #[serde(alias = "forceDelete")]
    force_delete: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListExamSubmissionsQuery {
    #[serde(default)]
    status: Option<SubmissionStatus>,
    #[serde(default)]
    skip: i64,
    #[serde(default = "default_limit")]
    limit: i64,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_exam).get(list_exams))
        .route("/:exam_id", get(get_exam).patch(update_exam).delete(delete_exam))
        .route("/:exam_id/publish", post(publish_exam))
        .route("/:exam_id/task-types", post(add_task_type))
        .route("/:exam_id/submissions", get(list_exam_submissions))
}

async fn create_exam(
    CurrentTeacher(teacher): CurrentTeacher,
    state: axum::extract::State<AppState>,
    Json(payload): Json<ExamCreate>,
) -> Result<(axum::http::StatusCode, Json<ExamResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    if payload.end_time <= payload.start_time {
        return Err(ApiError::BadRequest("end_time must be after start_time".to_string()));
    }

    let start_time = to_primitive_utc(payload.start_time);
    let end_time = to_primitive_utc(payload.end_time);

    let now = now_primitive();
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
        RETURNING {}", repositories::exams::COLUMNS,
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

    let task_types = insert_task_types(&mut tx, &exam.id, payload.task_types).await?;
    tx.commit()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(exam_to_response(exam, task_types)),
    ))
}

async fn list_exams(
    CurrentUser(user): CurrentUser,
    state: axum::extract::State<AppState>,
    Query(params): Query<ListExamsQuery>,
) -> Result<Json<Vec<ExamSummaryResponse>>, ApiError> {
    let skip = params.skip.max(0);
    let limit = params.limit.clamp(1, 1000);

    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT e.id, e.title, e.start_time, e.end_time, e.duration_minutes, e.status,
                COALESCE(tc.cnt, 0) AS task_count,
                COALESCE(sc.cnt, 0) AS student_count,
                COALESCE(pc.cnt, 0) AS pending_count
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

    for row in rows {
        let exam_id: String =
            row.try_get("id").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let title: String =
            row.try_get("title").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let start_time: PrimitiveDateTime =
            row.try_get("start_time").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let end_time: PrimitiveDateTime =
            row.try_get("end_time").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let duration_minutes: i32 = row
            .try_get("duration_minutes")
            .map_err(|e| ApiError::internal(e, "Bad row"))?;
        let status: ExamStatus =
            row.try_get("status").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let task_count: i64 =
            row.try_get("task_count").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let student_count: i64 =
            row.try_get("student_count").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let pending_count: i64 =
            row.try_get("pending_count").map_err(|e| ApiError::internal(e, "Bad row"))?;

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

    Ok(Json(summaries))
}

async fn get_exam(
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
        && !matches!(exam.status, ExamStatus::Published | ExamStatus::Active)
    {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let task_types = fetch_task_types(state.db(), &exam.id).await?;

    Ok(Json(exam_to_response(exam, task_types)))
}

async fn update_exam(
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

    if exam.created_by != teacher.id {
        return Err(ApiError::Forbidden("You can only update your own exams"));
    }

    payload
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Validate time constraints when either start or end time is updated
    let effective_start = payload.start_time.unwrap_or(exam.start_time.assume_utc());
    let effective_end = payload.end_time.unwrap_or(exam.end_time.assume_utc());
    if effective_end <= effective_start {
        return Err(ApiError::BadRequest("end_time must be after start_time".to_string()));
    }

    let now = now_primitive();
    let start_time = payload.start_time.map(to_primitive_utc);
    let end_time = payload.end_time.map(to_primitive_utc);

    sqlx::query(
        "UPDATE exams SET
            title = COALESCE($1, title),
            description = COALESCE($2, description),
            start_time = COALESCE($3, start_time),
            end_time = COALESCE($4, end_time),
            duration_minutes = COALESCE($5, duration_minutes),
            settings = COALESCE($6, settings),
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

    let task_types = fetch_task_types(state.db(), &updated.id).await?;

    Ok(Json(exam_to_response(updated, task_types)))
}

async fn delete_exam(
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

    if exam.created_by != teacher.id {
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

async fn publish_exam(
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

    if exam.created_by != teacher.id {
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

    let now = now_primitive();
    repositories::exams::publish(state.db(), &exam_id, now)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to publish exam"))?;

    let updated = repositories::exams::fetch_one_by_id(state.db(), &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch updated exam"))?;

    let task_types = fetch_task_types(state.db(), &updated.id).await?;

    tracing::info!(
        teacher_id = %teacher.id,
        exam_id = %updated.id,
        action = "exam_publish",
        "Exam published"
    );

    Ok(Json(exam_to_response(updated, task_types)))
}

async fn add_task_type(
    axum::extract::Path(exam_id): axum::extract::Path<String>,
    CurrentTeacher(_teacher): CurrentTeacher,
    state: axum::extract::State<AppState>,
    Json(payload): Json<TaskTypeCreate>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start transaction"))?;

    let exam_exists = repositories::exams::exists_by_id(&mut *tx, &exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch exam"))?;

    if exam_exists.is_none() {
        return Err(ApiError::NotFound("Exam not found".to_string()));
    }

    let now = now_primitive();
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

    insert_variants(&mut tx, &task_type_id, payload.variants).await?;
    tx.commit()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to commit transaction"))?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({
            "message": "Task type added successfully",
            "task_type_id": task_type_id
        })),
    ))
}

async fn list_exam_submissions(
    axum::extract::Path(exam_id): axum::extract::Path<String>,
    Query(params): Query<ListExamSubmissionsQuery>,
    CurrentTeacher(_teacher): CurrentTeacher,
    state: axum::extract::State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let mut query = String::from(
        "SELECT s.id, s.student_id, u.isu, u.full_name, s.submitted_at, s.status,
                s.ai_score, s.final_score, s.max_score
         FROM submissions s
         JOIN exam_sessions es ON s.session_id = es.id
         JOIN users u ON u.id = s.student_id
         WHERE es.exam_id = $1",
    );

    if params.status.is_some() {
        query.push_str(" AND s.status = $2");
    }

    let skip = params.skip.max(0);
    let limit = params.limit.clamp(1, 1000);
    query.push_str(" ORDER BY s.submitted_at DESC");
    query.push_str(&format!(" OFFSET {skip} LIMIT {limit}"));

    let mut sql = sqlx::query(&query).bind(&exam_id);
    if let Some(status) = params.status {
        sql = sql.bind(status);
    }

    let rows = sql
        .fetch_all(state.db())
        .await
        .map_err(|e| ApiError::internal(e, "Failed to list submissions"))?;

    let mut response = Vec::new();
    for row in rows {
        let submission_id: String =
            row.try_get("id").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let student_id: String =
            row.try_get("student_id").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let student_isu: String =
            row.try_get("isu").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let student_name: String =
            row.try_get("full_name").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let submitted_at: PrimitiveDateTime =
            row.try_get("submitted_at").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let status: SubmissionStatus =
            row.try_get("status").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let ai_score: Option<f64> =
            row.try_get("ai_score").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let final_score: Option<f64> =
            row.try_get("final_score").map_err(|e| ApiError::internal(e, "Bad row"))?;
        let max_score: f64 =
            row.try_get("max_score").map_err(|e| ApiError::internal(e, "Bad row"))?;

        response.push(serde_json::json!({
            "id": submission_id,
            "student_id": student_id,
            "student_isu": student_isu,
            "student_name": student_name,
            "submitted_at": format_primitive(submitted_at),
            "status": status,
            "ai_score": ai_score,
            "final_score": final_score,
            "max_score": max_score,
        }));
    }

    Ok(Json(response))
}

async fn insert_task_types(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    exam_id: &str,
    task_types: Vec<TaskTypeCreate>,
) -> Result<Vec<TaskTypeResponse>, ApiError> {
    let mut responses = Vec::new();
    let now = now_primitive();

    for task_type in task_types {
        let task_type_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO task_types (
                id, exam_id, title, description, order_index, max_score, rubric,
                difficulty, taxonomy_tags, formulas, units, validation_rules,
                created_at, updated_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(&task_type_id)
        .bind(exam_id)
        .bind(&task_type.title)
        .bind(&task_type.description)
        .bind(task_type.order_index)
        .bind(task_type.max_score)
        .bind(SqlxJson(task_type.rubric.clone()))
        .bind(task_type.difficulty)
        .bind(SqlxJson(task_type.taxonomy_tags.clone()))
        .bind(SqlxJson(task_type.formulas.clone()))
        .bind(SqlxJson(task_type.units.clone()))
        .bind(SqlxJson(task_type.validation_rules.clone()))
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to create task type"))?;

        let variants = insert_variants(tx, &task_type_id, task_type.variants).await?;

        responses.push(TaskTypeResponse {
            id: task_type_id,
            exam_id: exam_id.to_string(),
            title: task_type.title,
            description: task_type.description,
            order_index: task_type.order_index,
            max_score: task_type.max_score,
            rubric: task_type.rubric,
            difficulty: task_type.difficulty,
            taxonomy_tags: task_type.taxonomy_tags,
            formulas: task_type.formulas,
            units: task_type.units,
            validation_rules: task_type.validation_rules,
            created_at: format_primitive(now),
            updated_at: format_primitive(now),
            variants,
        });
    }

    Ok(responses)
}

async fn insert_variants(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_type_id: &str,
    variants: Vec<TaskVariantCreate>,
) -> Result<Vec<TaskVariantResponse>, ApiError> {
    let mut responses = Vec::new();
    let now = now_primitive();

    for variant in variants {
        let variant_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO task_variants (
                id, task_type_id, content, parameters, reference_solution,
                reference_answer, answer_tolerance, attachments, created_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(&variant_id)
        .bind(task_type_id)
        .bind(&variant.content)
        .bind(SqlxJson(variant.parameters.clone()))
        .bind(variant.reference_solution.clone())
        .bind(variant.reference_answer.clone())
        .bind(variant.answer_tolerance)
        .bind(SqlxJson(variant.attachments.clone()))
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to create task variant"))?;

        responses.push(TaskVariantResponse {
            id: variant_id,
            task_type_id: task_type_id.to_string(),
            content: variant.content,
            parameters: variant.parameters,
            reference_solution: variant.reference_solution,
            reference_answer: variant.reference_answer,
            answer_tolerance: variant.answer_tolerance,
            attachments: variant.attachments,
            created_at: format_primitive(now),
        });
    }

    Ok(responses)
}

async fn fetch_task_types(
    pool: &sqlx::PgPool,
    exam_id: &str,
) -> Result<Vec<TaskTypeResponse>, ApiError> {
    let task_types = repositories::task_types::list_by_exam(pool, exam_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch task types"))?;

    let mut responses = Vec::new();
    for task_type in task_types {
        let variants = repositories::task_types::list_variants(pool, &task_type.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to fetch variants"))?;

        let variant_responses = variants
            .into_iter()
            .map(|variant| TaskVariantResponse {
                id: variant.id,
                task_type_id: variant.task_type_id,
                content: variant.content,
                parameters: variant.parameters.0,
                reference_solution: variant.reference_solution,
                reference_answer: variant.reference_answer,
                answer_tolerance: variant.answer_tolerance,
                attachments: variant.attachments.0,
                created_at: format_primitive(variant.created_at),
            })
            .collect();

        responses.push(TaskTypeResponse {
            id: task_type.id,
            exam_id: task_type.exam_id,
            title: task_type.title,
            description: task_type.description,
            order_index: task_type.order_index,
            max_score: task_type.max_score,
            rubric: task_type.rubric.0,
            difficulty: task_type.difficulty,
            taxonomy_tags: task_type.taxonomy_tags.0,
            formulas: task_type.formulas.0,
            units: task_type.units.0,
            validation_rules: task_type.validation_rules.0,
            created_at: format_primitive(task_type.created_at),
            updated_at: format_primitive(task_type.updated_at),
            variants: variant_responses,
        });
    }

    Ok(responses)
}

fn exam_to_response(exam: Exam, task_types: Vec<TaskTypeResponse>) -> ExamResponse {
    ExamResponse {
        id: exam.id,
        title: exam.title,
        description: exam.description,
        start_time: format_primitive(exam.start_time),
        end_time: format_primitive(exam.end_time),
        duration_minutes: exam.duration_minutes,
        timezone: exam.timezone,
        max_attempts: exam.max_attempts,
        allow_breaks: exam.allow_breaks,
        break_duration_minutes: exam.break_duration_minutes,
        auto_save_interval: exam.auto_save_interval,
        settings: exam.settings.0,
        status: exam.status,
        created_by: exam.created_by,
        created_at: format_primitive(exam.created_at),
        updated_at: format_primitive(exam.updated_at),
        published_at: exam.published_at.map(format_primitive),
        task_types,
    }
}

fn to_primitive_utc(value: OffsetDateTime) -> PrimitiveDateTime {
    let utc = value.to_offset(UtcOffset::UTC);
    PrimitiveDateTime::new(utc.date(), utc.time())
}

fn now_primitive() -> PrimitiveDateTime {
    let now = OffsetDateTime::now_utc();
    PrimitiveDateTime::new(now.date(), now.time())
}

fn default_limit() -> i64 {
    100
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use serde_json::json;
    use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
    use tower::ServiceExt;

    use crate::db::types::UserRole;
    use crate::test_support;

    fn exam_payload() -> serde_json::Value {
        let now = OffsetDateTime::now_utc().replace_nanosecond(0).expect("nanoseconds");
        let start_time = (now - Duration::hours(1)).format(&Rfc3339).unwrap();
        let end_time = (now + Duration::hours(2)).format(&Rfc3339).unwrap();

        json!({
            "title": "Chemistry midterm",
            "description": "Unit test exam",
            "start_time": start_time,
            "end_time": end_time,
            "duration_minutes": 60,
            "timezone": "UTC",
            "max_attempts": 1,
            "allow_breaks": false,
            "break_duration_minutes": 0,
            "auto_save_interval": 10,
            "settings": {},
            "task_types": [
                {
                    "title": "Task 1",
                    "description": "Solve the equation",
                    "order_index": 1,
                    "max_score": 10.0,
                    "rubric": {"criteria": []},
                    "difficulty": "easy",
                    "taxonomy_tags": [],
                    "formulas": [],
                    "units": [],
                    "validation_rules": {},
                    "variants": [
                        {
                            "content": "H2 + O2 -> ?",
                            "parameters": {},
                            "reference_solution": null,
                            "reference_answer": null,
                            "answer_tolerance": 0.01,
                            "attachments": []
                        }
                    ]
                }
            ]
        })
    }

    #[tokio::test]
    async fn teacher_can_create_publish_and_list_exam() {
        let ctx = test_support::setup_test_context().await;

        let teacher = test_support::insert_user(
            ctx.state.db(),
            "000002",
            "Teacher User",
            UserRole::Teacher,
            "teacher-pass",
        )
        .await;
        let token = test_support::bearer_token(&teacher.id, ctx.state.settings());

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                "/api/v1/exams",
                Some(&token),
                Some(exam_payload()),
            ))
            .await
            .expect("create exam");

        let status = response.status();
        let created = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::CREATED, "response: {created}");
        let exam_id = created["id"].as_str().expect("exam id").to_string();
        assert_eq!(created["status"], "draft");
        assert_eq!(created["task_types"].as_array().unwrap().len(), 1);

        let response = ctx
            .app
            .clone()
            .oneshot(test_support::json_request(
                Method::POST,
                &format!("/api/v1/exams/{exam_id}/publish"),
                Some(&token),
                None,
            ))
            .await
            .expect("publish exam");

        let status = response.status();
        let published = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {published}");
        assert_eq!(published["status"], "published");

        let response = ctx
            .app
            .oneshot(test_support::json_request(
                Method::GET,
                "/api/v1/exams?status=published",
                Some(&token),
                None,
            ))
            .await
            .expect("list exams");

        let status = response.status();
        let list = test_support::read_json(response).await;
        assert_eq!(status, StatusCode::OK, "response: {list}");
        let items = list.as_array().expect("exam list");
        assert!(items.iter().any(|item| item["id"] == exam_id));
    }
}
