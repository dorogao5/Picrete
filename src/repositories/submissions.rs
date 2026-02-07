use sqlx::types::Json;
use sqlx::PgPool;
use time::PrimitiveDateTime;

use crate::db::models::Submission;
use crate::db::types::SubmissionStatus;
use std::collections::HashMap;

pub(crate) const COLUMNS: &str = "\
    id, session_id, student_id, submitted_at, status, ai_score, final_score, max_score, \
    ai_analysis, ai_comments, ai_processed_at, ai_request_started_at, ai_request_completed_at, \
    ai_request_duration_seconds, ai_error, ai_retry_count, teacher_comments, reviewed_by, \
    reviewed_at, is_flagged, flag_reasons, anomaly_scores, files_hash, created_at, updated_at";

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct TeacherSubmissionDetails {
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) student_id: String,
    pub(crate) submitted_at: PrimitiveDateTime,
    pub(crate) status: SubmissionStatus,
    pub(crate) ai_score: Option<f64>,
    pub(crate) final_score: Option<f64>,
    pub(crate) max_score: f64,
    pub(crate) ai_analysis: Option<Json<serde_json::Value>>,
    pub(crate) ai_comments: Option<String>,
    pub(crate) teacher_comments: Option<String>,
    pub(crate) is_flagged: bool,
    pub(crate) flag_reasons: Json<Vec<String>>,
    pub(crate) reviewed_by: Option<String>,
    pub(crate) reviewed_at: Option<PrimitiveDateTime>,
    pub(crate) exam_id: String,
    pub(crate) exam_title: String,
    pub(crate) exam_created_by: String,
    pub(crate) variant_assignments: Json<HashMap<String, String>>,
    pub(crate) student_name: String,
    pub(crate) student_isu: String,
}

pub(crate) async fn find_by_id(pool: &PgPool, id: &str) -> Result<Option<Submission>, sqlx::Error> {
    sqlx::query_as::<_, Submission>(&format!("SELECT {COLUMNS} FROM submissions WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub(crate) async fn find_exam_creator_by_submission(
    pool: &PgPool,
    submission_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT e.created_by
         FROM submissions s
         JOIN exam_sessions es ON es.id = s.session_id
         JOIN exams e ON e.id = es.exam_id
         WHERE s.id = $1",
    )
    .bind(submission_id)
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

pub(crate) async fn find_teacher_details(
    pool: &PgPool,
    submission_id: &str,
) -> Result<Option<TeacherSubmissionDetails>, sqlx::Error> {
    sqlx::query_as::<_, TeacherSubmissionDetails>(
        "SELECT s.id,
                s.session_id,
                s.student_id,
                s.submitted_at,
                s.status,
                s.ai_score,
                s.final_score,
                s.max_score,
                s.ai_analysis,
                s.ai_comments,
                s.teacher_comments,
                s.is_flagged,
                s.flag_reasons,
                s.reviewed_by,
                s.reviewed_at,
                es.exam_id,
                e.title AS exam_title,
                e.created_by AS exam_created_by,
                es.variant_assignments,
                u.full_name AS student_name,
                u.isu AS student_isu
         FROM submissions s
         JOIN exam_sessions es ON es.id = s.session_id
         JOIN exams e ON e.id = es.exam_id
         JOIN users u ON u.id = s.student_id
         WHERE s.id = $1",
    )
    .bind(submission_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn list_by_sessions(
    pool: &PgPool,
    session_ids: &[String],
) -> Result<Vec<Submission>, sqlx::Error> {
    if session_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, Submission>(&format!(
        "SELECT {COLUMNS} FROM submissions WHERE session_id = ANY($1)"
    ))
    .bind(session_ids)
    .fetch_all(pool)
    .await
}

pub(crate) async fn fetch_one_by_id(pool: &PgPool, id: &str) -> Result<Submission, sqlx::Error> {
    sqlx::query_as::<_, Submission>(&format!("SELECT {COLUMNS} FROM submissions WHERE id = $1"))
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
pub(crate) async fn create_if_absent(
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
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
         ON CONFLICT (session_id) DO NOTHING",
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
    sqlx::query(
        "UPDATE submissions
         SET status = $1, submitted_at = $2, updated_at = $2
         WHERE session_id = $3
           AND status NOT IN ($4, $5, $6, $7, $8)",
    )
    .bind(status)
    .bind(submitted_at)
    .bind(session_id)
    .bind(SubmissionStatus::Processing)
    .bind(SubmissionStatus::Preliminary)
    .bind(SubmissionStatus::Approved)
    .bind(SubmissionStatus::Flagged)
    .bind(SubmissionStatus::Rejected)
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
