use sqlx::PgPool;
use time::PrimitiveDateTime;

use crate::db::models::Submission;
use crate::db::types::SubmissionStatus;

pub(crate) const COLUMNS: &str = "\
    id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score, \
    ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at, \
    ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by, \
    reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at";

pub(crate) async fn find_by_id(
    pool: &PgPool,
    id: &str,
) -> Result<Option<Submission>, sqlx::Error> {
    sqlx::query_as::<_, Submission>(&format!(
        "SELECT {COLUMNS} FROM submissions WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn find_by_session(
    pool: &PgPool,
    session_id: &str,
) -> Result<Option<Submission>, sqlx::Error> {
    sqlx::query_as::<_, Submission>(&format!(
        "SELECT {COLUMNS} FROM submissions WHERE session_id = $1"
    ))
    .bind(session_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn fetch_one_by_id(
    pool: &PgPool,
    id: &str,
) -> Result<Submission, sqlx::Error> {
    sqlx::query_as::<_, Submission>(&format!(
        "SELECT {COLUMNS} FROM submissions WHERE id = $1"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
}

pub(crate) async fn find_id_by_session(
    pool: &PgPool,
    session_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>("SELECT id FROM submissions WHERE session_id = $1")
        .bind(session_id)
        .fetch_optional(pool)
        .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn create(
    pool: &PgPool,
    id: &str,
    session_id: &str,
    student_id: &str,
    status: SubmissionStatus,
    max_score: f64,
    submitted_at: PrimitiveDateTime,
    now: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO submissions (id, session_id, student_id, status, max_score, submitted_at, created_at, updated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(session_id)
    .bind(student_id)
    .bind(status)
    .bind(max_score)
    .bind(submitted_at)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn update_status_by_session(
    pool: &PgPool,
    session_id: &str,
    status: SubmissionStatus,
    submitted_at: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE submissions SET status = $1, submitted_at = $2 WHERE session_id = $3")
        .bind(status)
        .bind(submitted_at)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn approve(
    pool: &PgPool,
    id: &str,
    ai_score: Option<f64>,
    teacher_comments: Option<String>,
    reviewed_by: String,
    now: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submissions SET status = $1, final_score = $2, teacher_comments = $3,
            reviewed_by = $4, reviewed_at = $5 WHERE id = $6",
    )
    .bind(SubmissionStatus::Approved)
    .bind(ai_score)
    .bind(teacher_comments)
    .bind(reviewed_by)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn override_score(
    pool: &PgPool,
    id: &str,
    final_score: f64,
    teacher_comments: String,
    reviewed_by: String,
    now: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submissions SET final_score = $1, teacher_comments = $2,
            status = $3, reviewed_by = $4, reviewed_at = $5 WHERE id = $6",
    )
    .bind(final_score)
    .bind(teacher_comments)
    .bind(SubmissionStatus::Approved)
    .bind(reviewed_by)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn queue_regrade(
    pool: &PgPool,
    id: &str,
    now: PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submissions SET status = $1,
            ai_retry_count = COALESCE(ai_retry_count,0) + 1,
            ai_error = NULL,
            ai_request_started_at = NULL,
            ai_request_completed_at = NULL,
            ai_request_duration_seconds = NULL,
            ai_processed_at = NULL,
            is_flagged = FALSE,
            flag_reasons = $2,
            updated_at = $3
         WHERE id = $4",
    )
    .bind(SubmissionStatus::Processing)
    .bind(sqlx::types::Json(Vec::<String>::new()))
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
