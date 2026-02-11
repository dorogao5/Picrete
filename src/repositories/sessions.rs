use sqlx::{PgPool, Postgres, QueryBuilder};
use time::PrimitiveDateTime;

use crate::db::models::ExamSession;
use crate::db::types::{SessionStatus, SubmissionStatus, WorkKind};

pub(crate) const COLUMNS: &str = "\
    id, course_id, exam_id, student_id, variant_seed, variant_assignments, \
    started_at, submitted_at, expires_at, status, attempt_number, \
    ip_address, user_agent, last_auto_save, auto_save_data, created_at, updated_at";

pub(crate) struct CreateSession<'a> {
    pub(crate) id: &'a str,
    pub(crate) course_id: &'a str,
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

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ActiveSessionDeadlineRow {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) started_at: PrimitiveDateTime,
    pub(crate) expires_at: PrimitiveDateTime,
    pub(crate) exam_end_time: Option<PrimitiveDateTime>,
    pub(crate) exam_kind: Option<WorkKind>,
    pub(crate) exam_duration_minutes: Option<i32>,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ActiveSessionForStudentRow {
    pub(crate) id: String,
    pub(crate) course_id: String,
    pub(crate) exam_title: String,
}

pub(crate) async fn find_by_id(
    pool: &PgPool,
    course_id: &str,
    id: &str,
) -> Result<Option<ExamSession>, sqlx::Error> {
    sqlx::query_as::<_, ExamSession>(&format!(
        "SELECT {COLUMNS}
         FROM exam_sessions
         WHERE course_id = $1 AND id = $2"
    ))
    .bind(course_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn fetch_one_by_id(
    pool: &PgPool,
    course_id: &str,
    id: &str,
) -> Result<ExamSession, sqlx::Error> {
    sqlx::query_as::<_, ExamSession>(&format!(
        "SELECT {COLUMNS}
         FROM exam_sessions
         WHERE course_id = $1 AND id = $2"
    ))
    .bind(course_id)
    .bind(id)
    .fetch_one(pool)
    .await
}

pub(crate) async fn find_active(
    executor: impl sqlx::PgExecutor<'_>,
    course_id: &str,
    exam_id: &str,
    student_id: &str,
) -> Result<Option<ExamSession>, sqlx::Error> {
    sqlx::query_as::<_, ExamSession>(&format!(
        "SELECT {COLUMNS} FROM exam_sessions \
         WHERE course_id = $1 AND exam_id = $2 AND student_id = $3 AND status = $4"
    ))
    .bind(course_id)
    .bind(exam_id)
    .bind(student_id)
    .bind(SessionStatus::Active)
    .fetch_optional(executor)
    .await
}

pub(crate) async fn count_by_exam_and_student(
    executor: impl sqlx::PgExecutor<'_>,
    course_id: &str,
    exam_id: &str,
    student_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM exam_sessions
         WHERE course_id = $1 AND exam_id = $2 AND student_id = $3",
    )
    .bind(course_id)
    .bind(exam_id)
    .bind(student_id)
    .fetch_one(executor)
    .await
}

pub(crate) async fn list_by_student(
    pool: &PgPool,
    course_id: &str,
    student_id: &str,
    status: Option<SubmissionStatus>,
    skip: i64,
    limit: i64,
) -> Result<Vec<ExamSession>, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new(format!(
        "SELECT {COLUMNS}
         FROM exam_sessions
         WHERE course_id = "
    ));
    builder.push_bind(course_id);
    builder.push(" AND student_id = ");
    builder.push_bind(student_id);

    if let Some(status) = status {
        builder.push(
            " AND id IN (
                SELECT session_id
                FROM submissions
                WHERE course_id = ",
        );
        builder.push_bind(course_id);
        builder.push(" AND status = ");
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
    course_id: &str,
    student_id: &str,
    status: Option<SubmissionStatus>,
) -> Result<i64, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT COUNT(*)
         FROM exam_sessions
         WHERE course_id = ",
    );
    builder.push_bind(course_id);
    builder.push(" AND student_id = ");
    builder.push_bind(student_id);

    if let Some(status) = status {
        builder.push(
            " AND id IN (
                SELECT session_id
                FROM submissions
                WHERE course_id = ",
        );
        builder.push_bind(course_id);
        builder.push(" AND status = ");
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

pub(crate) async fn acquire_exam_user_lock(
    executor: impl sqlx::PgExecutor<'_>,
    course_id: &str,
    exam_id: &str,
    student_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1), hashtext($2 || ':' || $3))")
        .bind(course_id)
        .bind(exam_id)
        .bind(student_id)
        .execute(executor)
        .await?;
    Ok(())
}

pub(crate) async fn acquire_global_lock(
    executor: impl sqlx::PgExecutor<'_>,
    lock_key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(lock_key)
        .execute(executor)
        .await?;
    Ok(())
}

pub(crate) async fn create(
    executor: impl sqlx::PgExecutor<'_>,
    session: CreateSession<'_>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO exam_sessions (
            id, course_id, exam_id, student_id, variant_seed, variant_assignments,
            started_at, expires_at, status, attempt_number, created_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
        ON CONFLICT DO NOTHING",
    )
    .bind(session.id)
    .bind(session.course_id)
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

pub(crate) async fn update_auto_save(
    pool: &PgPool,
    course_id: &str,
    id: &str,
    data: serde_json::Value,
    now: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE exam_sessions
         SET auto_save_data = $1, last_auto_save = $2
         WHERE course_id = $3 AND id = $4",
    )
    .bind(data)
    .bind(now)
    .bind(course_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn submit(
    pool: &PgPool,
    course_id: &str,
    id: &str,
    now: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE exam_sessions
         SET status = $1, submitted_at = $2
         WHERE course_id = $3 AND id = $4",
    )
    .bind(SessionStatus::Submitted)
    .bind(now)
    .bind(course_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn list_active_with_exam_end(
    pool: &PgPool,
) -> Result<Vec<ActiveSessionDeadlineRow>, sqlx::Error> {
    sqlx::query_as::<_, ActiveSessionDeadlineRow>(
        "SELECT s.id,
                s.course_id,
                s.started_at,
                s.expires_at,
                e.end_time AS exam_end_time,
                e.kind AS exam_kind,
                e.duration_minutes AS exam_duration_minutes
         FROM exam_sessions s
         LEFT JOIN exams e ON e.course_id = s.course_id AND e.id = s.exam_id
         WHERE s.status = $1",
    )
    .bind(SessionStatus::Active)
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_active_by_student(
    pool: &PgPool,
    student_id: &str,
) -> Result<Vec<ActiveSessionForStudentRow>, sqlx::Error> {
    sqlx::query_as::<_, ActiveSessionForStudentRow>(
        "SELECT s.id,
                s.course_id,
                e.title AS exam_title
         FROM exam_sessions s
         JOIN exams e ON e.course_id = s.course_id AND e.id = s.exam_id
         WHERE s.student_id = $1
           AND s.status = $2
         ORDER BY s.started_at DESC",
    )
    .bind(student_id)
    .bind(SessionStatus::Active)
    .fetch_all(pool)
    .await
}

pub(crate) async fn expire_with_deadline(
    pool: &PgPool,
    course_id: &str,
    id: &str,
    hard_deadline: PrimitiveDateTime,
    updated_at: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE exam_sessions
         SET status = $1,
             submitted_at = COALESCE(submitted_at, $2),
             updated_at = $3
         WHERE course_id = $4 AND id = $5 AND status = $6",
    )
    .bind(SessionStatus::Expired)
    .bind(hard_deadline)
    .bind(updated_at)
    .bind(course_id)
    .bind(id)
    .bind(SessionStatus::Active)
    .execute(pool)
    .await?;
    Ok(())
}
