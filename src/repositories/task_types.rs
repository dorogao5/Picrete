use sqlx::types::Json as SqlxJson;
use sqlx::PgPool;
use time::PrimitiveDateTime;

use crate::db::models::{TaskType, TaskVariant};
use crate::db::types::DifficultyLevel;

pub(crate) const COLUMNS: &str = "\
    id, exam_id, title, description, order_index, max_score, rubric, \
    difficulty, taxonomy_tags, formulas, units, validation_rules, \
    created_at, updated_at";

pub(crate) const VARIANT_COLUMNS: &str = "\
    id, task_type_id, content, parameters, reference_solution, \
    reference_answer, answer_tolerance, attachments, created_at";

pub(crate) async fn list_by_exam(
    pool: &PgPool,
    exam_id: &str,
) -> Result<Vec<TaskType>, sqlx::Error> {
    sqlx::query_as::<_, TaskType>(&format!(
        "SELECT {COLUMNS} FROM task_types WHERE exam_id = $1 ORDER BY order_index"
    ))
    .bind(exam_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_variants(
    pool: &PgPool,
    task_type_id: &str,
) -> Result<Vec<TaskVariant>, sqlx::Error> {
    sqlx::query_as::<_, TaskVariant>(&format!(
        "SELECT {VARIANT_COLUMNS} FROM task_variants WHERE task_type_id = $1"
    ))
    .bind(task_type_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_variants_by_task_type_ids(
    pool: &PgPool,
    task_type_ids: &[String],
) -> Result<Vec<TaskVariant>, sqlx::Error> {
    if task_type_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, TaskVariant>(&format!(
        "SELECT {VARIANT_COLUMNS} FROM task_variants WHERE task_type_id = ANY($1)"
    ))
    .bind(task_type_ids)
    .fetch_all(pool)
    .await
}

pub(crate) struct CreateTaskType<'a> {
    pub(crate) id: &'a str,
    pub(crate) exam_id: &'a str,
    pub(crate) title: &'a str,
    pub(crate) description: &'a str,
    pub(crate) order_index: i32,
    pub(crate) max_score: f64,
    pub(crate) rubric: serde_json::Value,
    pub(crate) difficulty: DifficultyLevel,
    pub(crate) taxonomy_tags: Vec<String>,
    pub(crate) formulas: Vec<String>,
    pub(crate) units: Vec<serde_json::Value>,
    pub(crate) validation_rules: serde_json::Value,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

pub(crate) async fn create(
    executor: impl sqlx::PgExecutor<'_>,
    params: CreateTaskType<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO task_types (
            id, exam_id, title, description, order_index, max_score, rubric,
            difficulty, taxonomy_tags, formulas, units, validation_rules,
            created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
    )
    .bind(params.id)
    .bind(params.exam_id)
    .bind(params.title)
    .bind(params.description)
    .bind(params.order_index)
    .bind(params.max_score)
    .bind(SqlxJson(params.rubric))
    .bind(params.difficulty)
    .bind(SqlxJson(params.taxonomy_tags))
    .bind(SqlxJson(params.formulas))
    .bind(SqlxJson(params.units))
    .bind(SqlxJson(params.validation_rules))
    .bind(params.created_at)
    .bind(params.updated_at)
    .execute(executor)
    .await?;
    Ok(())
}

pub(crate) struct CreateTaskVariant<'a> {
    pub(crate) id: &'a str,
    pub(crate) task_type_id: &'a str,
    pub(crate) content: &'a str,
    pub(crate) parameters: serde_json::Value,
    pub(crate) reference_solution: Option<String>,
    pub(crate) reference_answer: Option<String>,
    pub(crate) answer_tolerance: f64,
    pub(crate) attachments: Vec<String>,
    pub(crate) created_at: PrimitiveDateTime,
}

pub(crate) async fn create_variant(
    executor: impl sqlx::PgExecutor<'_>,
    params: CreateTaskVariant<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO task_variants (
            id, task_type_id, content, parameters, reference_solution,
            reference_answer, answer_tolerance, attachments, created_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(params.id)
    .bind(params.task_type_id)
    .bind(params.content)
    .bind(SqlxJson(params.parameters))
    .bind(params.reference_solution)
    .bind(params.reference_answer)
    .bind(params.answer_tolerance)
    .bind(SqlxJson(params.attachments))
    .bind(params.created_at)
    .execute(executor)
    .await?;
    Ok(())
}
