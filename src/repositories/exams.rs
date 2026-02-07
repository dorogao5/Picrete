use sqlx::PgPool;

use crate::db::models::Exam;
use crate::db::types::ExamStatus;

pub(crate) const COLUMNS: &str = "\
    id, title, description, start_time, end_time, duration_minutes, timezone, \
    max_attempts, allow_breaks, break_duration_minutes, auto_save_interval, \
    status, created_by, created_at, updated_at, published_at, settings";

pub(crate) async fn find_by_id(pool: &PgPool, id: &str) -> Result<Option<Exam>, sqlx::Error> {
    sqlx::query_as::<_, Exam>(&format!("SELECT {COLUMNS} FROM exams WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn fetch_one_by_id(pool: &PgPool, id: &str) -> Result<Exam, sqlx::Error> {
    sqlx::query_as::<_, Exam>(&format!("SELECT {COLUMNS} FROM exams WHERE id = $1"))
        .bind(id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn count_task_types(pool: &PgPool, exam_id: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM task_types WHERE exam_id = $1")
        .bind(exam_id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn count_sessions(pool: &PgPool, exam_id: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM exam_sessions WHERE exam_id = $1")
        .bind(exam_id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn delete_by_id(pool: &PgPool, id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM exams WHERE id = $1").bind(id).execute(pool).await?;
    Ok(())
}

pub(crate) async fn publish(
    pool: &PgPool,
    id: &str,
    now: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE exams SET status = $1, published_at = $2, updated_at = $3 WHERE id = $4")
        .bind(ExamStatus::Published)
        .bind(now)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn find_title_by_id(
    pool: &PgPool,
    id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT title FROM exams WHERE id = $1").bind(id).fetch_optional(pool).await
}

pub(crate) async fn max_score_for_exam(pool: &PgPool, exam_id: &str) -> f64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(max_score), 100) FROM task_types WHERE exam_id = $1")
        .bind(exam_id)
        .fetch_one(pool)
        .await
        .unwrap_or(100.0)
}
