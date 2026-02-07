use sqlx::{PgPool, Postgres, QueryBuilder};

use crate::db::models::ExamSession;
use crate::db::types::{SessionStatus, SubmissionStatus};

pub(crate) const COLUMNS: &str = "\
    id, exam_id, student_id, variant_seed, variant_assignments, \
    started_at, submitted_at, expires_at, status, attempt_number, \
    ip_address, user_agent, last_auto_save, auto_save_data, created_at, updated_at";

pub(crate) struct CreateSession<'a> {
    pub(crate) id: &'a str,
    pub(crate) exam_id: &'a str,
    pub(crate) student_id: &'a str,
    pub(crate) variant_seed: i32,
    pub(crate) variant_assignments: serde_json::Value,
    pub(crate) started_at: time::PrimitiveDateTime,
    pub(crate) expires_at: time::PrimitiveDateTime,
    pub(crate) status: SessionStatus,
    pub(crate) attempt_number: i32,
    pub(crate) created_at: time::PrimitiveDateTime,
    pub(crate) updated_at: time::PrimitiveDateTime,
}

pub(crate) async fn find_by_id(
    pool: &PgPool,
    id: &str,
) -> Result<Option<ExamSession>, sqlx::Error> {
    sqlx::query_as::<_, ExamSession>(&format!("SELECT {COLUMNS} FROM exam_sessions WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn fetch_one_by_id(pool: &PgPool, id: &str) -> Result<ExamSession, sqlx::Error> {
    sqlx::query_as::<_, ExamSession>(&format!("SELECT {COLUMNS} FROM exam_sessions WHERE id = $1"))
        .bind(id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn find_active(
    executor: impl sqlx::PgExecutor<'_>,
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
    .fetch_optional(executor)
    .await
}

pub(crate) async fn count_by_exam_and_student(
    executor: impl sqlx::PgExecutor<'_>,
    exam_id: &str,
    student_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM exam_sessions WHERE exam_id = $1 AND student_id = $2")
        .bind(exam_id)
        .bind(student_id)
        .fetch_one(executor)
        .await
}

pub(crate) async fn list_by_student(
    pool: &PgPool,
    student_id: &str,
    status: Option<SubmissionStatus>,
    skip: i64,
    limit: i64,
) -> Result<Vec<ExamSession>, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new(format!(
        "SELECT {COLUMNS} FROM exam_sessions WHERE student_id = "
    ));
    builder.push_bind(student_id);

    if let Some(status) = status {
        builder.push(" AND id IN (SELECT session_id FROM submissions WHERE status = ");
        builder.push_bind(status);
        builder.push(")");
    }

    builder.push(" ORDER BY created_at DESC OFFSET ");
    builder.push_bind(skip.max(0));
    builder.push(" LIMIT ");
    builder.push_bind(limit.clamp(1, 1000));

    builder.build_query_as::<ExamSession>().fetch_all(pool).await
}

pub(crate) async fn count_by_student(
    pool: &PgPool,
    student_id: &str,
    status: Option<SubmissionStatus>,
) -> Result<i64, sqlx::Error> {
    let mut builder =
        QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM exam_sessions WHERE student_id = ");
    builder.push_bind(student_id);

    if let Some(status) = status {
        builder.push(" AND id IN (SELECT session_id FROM submissions WHERE status = ");
        builder.push_bind(status);
        builder.push(")");
    }

    builder.build_query_scalar::<i64>().fetch_one(pool).await
}

pub(crate) async fn count_active(executor: impl sqlx::PgExecutor<'_>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM exam_sessions WHERE status = $1")
        .bind(SessionStatus::Active)
        .fetch_one(executor)
        .await
}

pub(crate) async fn create(
    executor: impl sqlx::PgExecutor<'_>,
    session: CreateSession<'_>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO exam_sessions (
            id, exam_id, student_id, variant_seed, variant_assignments,
            started_at, expires_at, status, attempt_number, created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
        ON CONFLICT DO NOTHING",
    )
    .bind(session.id)
    .bind(session.exam_id)
    .bind(session.student_id)
    .bind(session.variant_seed)
    .bind(session.variant_assignments)
    .bind(session.started_at)
    .bind(session.expires_at)
    .bind(session.status)
    .bind(session.attempt_number)
    .bind(session.created_at)
    .bind(session.updated_at)
    .execute(executor)
    .await?;

    Ok(result.rows_affected() > 0)
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
