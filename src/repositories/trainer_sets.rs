use sqlx::types::Json as SqlxJson;
use sqlx::PgPool;
use time::PrimitiveDateTime;

use crate::db::models::TrainerSet;

pub(crate) const PHYSICAL_CHEMISTRY_SOURCE: &str = "studio_fizicheskaya_himiya";

pub(crate) async fn uses_studio_generation(pool: &PgPool, course_id: &str, source: &str) -> Result<bool, sqlx::Error> {
    if source == PHYSICAL_CHEMISTRY_SOURCE { return Ok(true); }
    let assistant = super::course_ai_assistants::find(pool, course_id).await?;
    Ok(assistant.is_some_and(|a| a.enabled &&
        a.snapshot.pointer("/assistant/runtime_policy/generation_policy").and_then(serde_json::Value::as_str) == Some("single_verifier")))
}

// Deliberately independent of release and difficulty: republishing and changing
// levels must not erase an earned unlock or count the same task twice.
pub(crate) async fn generation_solved_count(
    pool: &PgPool,
    course_id: &str,
    student_id: &str,
    trainer_id: &str,
    section_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(DISTINCT a.task_id) FROM practice_attempts a
         JOIN task_bank_items i ON i.id=a.task_id
         JOIN task_bank_sources s ON s.id=i.source_id
         WHERE a.course_id=$1 AND a.student_id=$2 AND a.trainer_id=$3
           AND a.section_id=$4 AND a.solved AND NOT a.preview
           AND EXISTS (SELECT 1 FROM course_trainers t,
             LATERAL jsonb_array_elements(t.published->'sections') sec,
             LATERAL jsonb_array_elements(sec->'items') item
             WHERE t.id=$3 AND t.course_id=$1 AND sec->>'id'=$4 AND item->>'task_id'=a.task_id)",
    )
    .bind(course_id)
    .bind(student_id)
    .bind(trainer_id)
    .bind(section_id)
    .fetch_one(pool)
    .await
}

pub(crate) const TRAINER_SET_COLUMNS: &str = "\
    id, student_id, course_id, title, source_id, filters, is_deleted, created_at, updated_at";

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct TrainerSetSummaryRow {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) source_code: String,
    pub(crate) source_title: String,
    pub(crate) filters: SqlxJson<serde_json::Value>,
    pub(crate) item_count: i64,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
    pub(crate) total_count: i64,
}

pub(crate) struct CreateTrainerSet<'a> {
    pub(crate) id: &'a str,
    pub(crate) student_id: &'a str,
    pub(crate) course_id: &'a str,
    pub(crate) title: &'a str,
    pub(crate) source_id: &'a str,
    pub(crate) filters: serde_json::Value,
    pub(crate) now: PrimitiveDateTime,
}

pub(crate) async fn create(
    executor: impl sqlx::PgExecutor<'_>,
    params: CreateTrainerSet<'_>,
) -> Result<TrainerSet, sqlx::Error> {
    sqlx::query_as::<_, TrainerSet>(&format!(
        "INSERT INTO trainer_sets (
            id, student_id, course_id, title, source_id, filters, is_deleted, created_at, updated_at
         ) VALUES ($1,$2,$3,$4,$5,$6,FALSE,$7,$8)
         RETURNING {TRAINER_SET_COLUMNS}"
    ))
    .bind(params.id)
    .bind(params.student_id)
    .bind(params.course_id)
    .bind(params.title)
    .bind(params.source_id)
    .bind(SqlxJson(params.filters))
    .bind(params.now)
    .bind(params.now)
    .fetch_one(executor)
    .await
}

pub(crate) async fn insert_items(
    executor: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    trainer_set_id: &str,
    item_ids: &[String],
) -> Result<(), sqlx::Error> {
    for (order_index, item_id) in item_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO trainer_set_items (trainer_set_id, task_bank_item_id, order_index)
             VALUES ($1,$2,$3)",
        )
        .bind(trainer_set_id)
        .bind(item_id)
        .bind(order_index as i32)
        .execute(&mut **executor)
        .await?;
    }

    Ok(())
}

pub(crate) struct ListTrainerSetsParams {
    pub(crate) course_id: String,
    pub(crate) student_id: String,
    pub(crate) skip: i64,
    pub(crate) limit: i64,
}

pub(crate) async fn list_for_student(
    pool: &PgPool,
    params: ListTrainerSetsParams,
) -> Result<Vec<TrainerSetSummaryRow>, sqlx::Error> {
    sqlx::query_as::<_, TrainerSetSummaryRow>(
        "SELECT ts.id,
                ts.title,
                s.code AS source_code,
                s.title AS source_title,
                ts.filters,
                COALESCE(items.item_count, 0) AS item_count,
                ts.created_at,
                ts.updated_at,
                COUNT(*) OVER() AS total_count
         FROM trainer_sets ts
         JOIN task_bank_sources s ON s.id = ts.source_id
         LEFT JOIN (
             SELECT trainer_set_id, COUNT(*) AS item_count
             FROM trainer_set_items
             GROUP BY trainer_set_id
         ) items ON items.trainer_set_id = ts.id
         WHERE ts.course_id = $1
           AND ts.student_id = $2
           AND ts.is_deleted = FALSE
         ORDER BY ts.created_at DESC
         OFFSET $3
         LIMIT $4",
    )
    .bind(params.course_id)
    .bind(params.student_id)
    .bind(params.skip.max(0))
    .bind(params.limit.clamp(1, 1000))
    .fetch_all(pool)
    .await
}

pub(crate) async fn find_for_student(
    pool: &PgPool,
    course_id: &str,
    student_id: &str,
    trainer_set_id: &str,
) -> Result<Option<TrainerSet>, sqlx::Error> {
    sqlx::query_as::<_, TrainerSet>(&format!(
        "SELECT {TRAINER_SET_COLUMNS}
         FROM trainer_sets
         WHERE id = $1
           AND course_id = $2
           AND student_id = $3
           AND is_deleted = FALSE"
    ))
    .bind(trainer_set_id)
    .bind(course_id)
    .bind(student_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn list_item_ids(
    pool: &PgPool,
    trainer_set_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT task_bank_item_id
         FROM trainer_set_items
         WHERE trainer_set_id = $1
         ORDER BY order_index",
    )
    .bind(trainer_set_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn soft_delete(
    pool: &PgPool,
    course_id: &str,
    student_id: &str,
    trainer_set_id: &str,
    now: PrimitiveDateTime,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE trainer_sets
         SET is_deleted = TRUE,
             updated_at = $1
         WHERE id = $2
           AND course_id = $3
           AND student_id = $4
           AND is_deleted = FALSE",
    )
    .bind(now)
    .bind(trainer_set_id)
    .bind(course_id)
    .bind(student_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
