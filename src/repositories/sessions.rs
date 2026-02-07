use sqlx::PgPool;

use crate::db::models::ExamSession;
use crate::db::types::SessionStatus;

pub(crate) const COLUMNS: &str = "\
    id, exam_id, student_id, variant_seed, variant_assignments, \
    started_at, submitted_at, expires_at, status, attempt_number, \
    ip_address, user_agent, last_auto_save, auto_save_data, created_at, updated_at";

pub(crate) async fn find_by_id(
    pool: &PgPool,
    id: &str,
) -> Result<Option<ExamSession>, sqlx::Error> {
    sqlx::query_as::<_, ExamSession>(&format!(
        "SELECT {COLUMNS} FROM exam_sessions WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn fetch_one_by_id(
    pool: &PgPool,
    id: &str,
) -> Result<ExamSession, sqlx::Error> {
    sqlx::query_as::<_, ExamSession>(&format!(
        "SELECT {COLUMNS} FROM exam_sessions WHERE id = $1"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
}

pub(crate) async fn find_active(
    pool: &PgPool,
    exam_id: &str,
    student_id: &str,
) -> Result<Option<ExamSession>, sqlx::Error> {
    sqlx::query_as::<_, ExamSession>(&format!(
        "SELECT {COLUMNS} FROM exam_sessions \
         WHERE exam_id = $1 AND student_id = $2 AND status = $3"
    ))
    .bind(exam_id)
    .bind(student_id)
    .bind(SessionStatus::Active)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn count_by_exam_and_student(
    pool: &PgPool,
    exam_id: &str,
    student_id: &str,
) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM exam_sessions WHERE exam_id = $1 AND student_id = $2")
        .bind(exam_id)
        .bind(student_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0)
}

pub(crate) async fn list_by_student(
    pool: &PgPool,
    student_id: &str,
) -> Result<Vec<ExamSession>, sqlx::Error> {
    sqlx::query_as::<_, ExamSession>(&format!(
        "SELECT {COLUMNS} FROM exam_sessions WHERE student_id = $1"
    ))
    .bind(student_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn update_status(
    pool: &PgPool,
    id: &str,
    status: SessionStatus,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE exam_sessions SET status = $1 WHERE id = $2")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn update_auto_save(
    pool: &PgPool,
    id: &str,
    data: serde_json::Value,
    now: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE exam_sessions SET auto_save_data = $1, last_auto_save = $2 WHERE id = $3")
        .bind(data)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn submit(
    pool: &PgPool,
    id: &str,
    now: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE exam_sessions SET status = $1, submitted_at = $2 WHERE id = $3")
        .bind(SessionStatus::Submitted)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
