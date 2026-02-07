use sqlx::PgPool;

use crate::db::models::{TaskType, TaskVariant};

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

pub(crate) async fn find_variant_by_id(
    pool: &PgPool,
    id: &str,
) -> Result<Option<TaskVariant>, sqlx::Error> {
    sqlx::query_as::<_, TaskVariant>(&format!(
        "SELECT {VARIANT_COLUMNS} FROM task_variants WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}
