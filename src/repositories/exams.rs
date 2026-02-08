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

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ExamSummaryRow {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) start_time: PrimitiveDateTime,
    pub(crate) end_time: PrimitiveDateTime,
    pub(crate) duration_minutes: i32,
    pub(crate) status: ExamStatus,
    pub(crate) task_count: i64,
    pub(crate) student_count: i64,
    pub(crate) pending_count: i64,
    pub(crate) total_count: i64,
}

pub(crate) struct CreateExam<'a> {
    pub(crate) id: &'a str,
    pub(crate) title: &'a str,
    pub(crate) description: Option<&'a str>,
    pub(crate) start_time: PrimitiveDateTime,
    pub(crate) end_time: PrimitiveDateTime,
    pub(crate) duration_minutes: i32,
    pub(crate) timezone: &'a str,
    pub(crate) max_attempts: i32,
    pub(crate) allow_breaks: bool,
    pub(crate) break_duration_minutes: i32,
    pub(crate) auto_save_interval: i32,
    pub(crate) status: ExamStatus,
    pub(crate) created_by: &'a str,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
    pub(crate) settings: serde_json::Value,
}

pub(crate) struct UpdateExam {
    pub(crate) title: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) start_time: Option<PrimitiveDateTime>,
    pub(crate) end_time: Option<PrimitiveDateTime>,
    pub(crate) duration_minutes: Option<i32>,
    pub(crate) settings: Option<serde_json::Value>,
    pub(crate) updated_at: PrimitiveDateTime,
}

pub(crate) struct ListExamSummariesParams {
    pub(crate) student_visible_only: bool,
    pub(crate) status: Option<ExamStatus>,
    pub(crate) skip: i64,
    pub(crate) limit: i64,
}

pub(crate) async fn create(
    executor: impl sqlx::PgExecutor<'_>,
    params: CreateExam<'_>,
) -> Result<Exam, sqlx::Error> {
    sqlx::query_as::<_, Exam>(&format!(
        "INSERT INTO exams (
            id, title, description, start_time, end_time, duration_minutes, timezone,
            max_attempts, allow_breaks, break_duration_minutes, auto_save_interval,
            status, created_by, created_at, updated_at, settings
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
        RETURNING {COLUMNS}",
    ))
    .bind(params.id)
    .bind(params.title)
    .bind(params.description)
    .bind(params.start_time)
    .bind(params.end_time)
    .bind(params.duration_minutes)
    .bind(params.timezone)
    .bind(params.max_attempts)
    .bind(params.allow_breaks)
    .bind(params.break_duration_minutes)
    .bind(params.auto_save_interval)
    .bind(params.status)
    .bind(params.created_by)
    .bind(params.created_at)
    .bind(params.updated_at)
    .bind(params.settings)
    .fetch_one(executor)
    .await
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

pub(crate) async fn list_summaries(
    pool: &PgPool,
    params: ListExamSummariesParams,
) -> Result<Vec<ExamSummaryRow>, sqlx::Error> {
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

    if params.student_visible_only {
        builder.push(" WHERE e.status IN (");
        builder.push_bind(ExamStatus::Published);
        builder.push(", ");
        builder.push_bind(ExamStatus::Active);
        builder.push(", ");
        builder.push_bind(ExamStatus::Completed);
        builder.push(")");
    }

    if let Some(status) = params.status {
        if params.student_visible_only {
            builder.push(" AND ");
        } else {
            builder.push(" WHERE ");
        }
        builder.push("e.status = ");
        builder.push_bind(status);
    }

    builder.push(" ORDER BY e.start_time DESC");
    builder.push(" OFFSET ");
    builder.push_bind(params.skip.max(0));
    builder.push(" LIMIT ");
    builder.push_bind(params.limit.clamp(1, 1000));

    builder.build_query_as::<ExamSummaryRow>().fetch_all(pool).await
}

pub(crate) async fn update(pool: &PgPool, id: &str, params: UpdateExam) -> Result<(), sqlx::Error> {
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
    .bind(params.title)
    .bind(params.description)
    .bind(params.start_time)
    .bind(params.end_time)
    .bind(params.duration_minutes)
    .bind(params.settings)
    .bind(params.updated_at)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
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

pub(crate) async fn list_ready_to_complete(
    pool: &PgPool,
    now: PrimitiveDateTime,
) -> Result<Vec<Exam>, sqlx::Error> {
    sqlx::query_as::<_, Exam>(&format!(
        "SELECT {COLUMNS}
         FROM exams
         WHERE status IN ($1, $2)
           AND end_time <= $3"
    ))
    .bind(ExamStatus::Active)
    .bind(ExamStatus::Published)
    .bind(now)
    .fetch_all(pool)
    .await
}

pub(crate) async fn mark_completed(
    pool: &PgPool,
    exam_id: &str,
    now: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE exams SET status = $1, updated_at = $2 WHERE id = $3")
        .bind(ExamStatus::Completed)
        .bind(now)
        .bind(exam_id)
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
