use sqlx::PgPool;
use sqlx::{Postgres, QueryBuilder};
use time::PrimitiveDateTime;

use crate::db::models::Exam;
use crate::db::types::{ExamStatus, SubmissionStatus};

pub(crate) const COLUMNS: &str = "\
    id, title, description, start_time, end_time, duration_minutes, timezone, \
    max_attempts, allow_breaks, break_duration_minutes, auto_save_interval, \
    status, created_by, created_at, updated_at, published_at, settings";

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ExamSubmissionRow {
    pub(crate) id: String,
    pub(crate) student_id: String,
    pub(crate) student_isu: String,
    pub(crate) student_name: String,
    pub(crate) submitted_at: PrimitiveDateTime,
    pub(crate) status: SubmissionStatus,
    pub(crate) ai_score: Option<f64>,
    pub(crate) final_score: Option<f64>,
    pub(crate) max_score: f64,
}

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

pub(crate) async fn list_titles_by_ids(
    pool: &PgPool,
    exam_ids: &[String],
) -> Result<Vec<(String, String)>, sqlx::Error> {
    if exam_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, (String, String)>("SELECT id, title FROM exams WHERE id = ANY($1)")
        .bind(exam_ids)
        .fetch_all(pool)
        .await
}

pub(crate) async fn max_score_for_exam(pool: &PgPool, exam_id: &str) -> Result<f64, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(SUM(max_score), 100) FROM task_types WHERE exam_id = $1")
        .bind(exam_id)
        .fetch_one(pool)
        .await
}

pub(crate) async fn list_submissions_by_exam(
    pool: &PgPool,
    exam_id: &str,
    status: Option<SubmissionStatus>,
    skip: i64,
    limit: i64,
) -> Result<Vec<ExamSubmissionRow>, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT s.id,
                s.student_id,
                u.isu AS student_isu,
                u.full_name AS student_name,
                s.submitted_at,
                s.status,
                s.ai_score,
                s.final_score,
                s.max_score
         FROM submissions s
         JOIN exam_sessions es ON s.session_id = es.id
         JOIN users u ON u.id = s.student_id
         WHERE es.exam_id = ",
    );
    builder.push_bind(exam_id);

    if let Some(status) = status {
        builder.push(" AND s.status = ");
        builder.push_bind(status);
    }

    builder.push(" ORDER BY s.submitted_at DESC OFFSET ");
    builder.push_bind(skip.max(0));
    builder.push(" LIMIT ");
    builder.push_bind(limit.clamp(1, 1000));

    builder.build_query_as::<ExamSubmissionRow>().fetch_all(pool).await
}

pub(crate) async fn count_submissions_by_exam(
    pool: &PgPool,
    exam_id: &str,
    status: Option<SubmissionStatus>,
) -> Result<i64, sqlx::Error> {
    let mut builder = QueryBuilder::<Postgres>::new(
        "SELECT COUNT(*)
         FROM submissions s
         JOIN exam_sessions es ON s.session_id = es.id
         WHERE es.exam_id = ",
    );
    builder.push_bind(exam_id);

    if let Some(status) = status {
        builder.push(" AND s.status = ");
        builder.push_bind(status);
    }

    builder.build_query_scalar::<i64>().fetch_one(pool).await
}
