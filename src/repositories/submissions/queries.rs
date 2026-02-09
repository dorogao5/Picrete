use sqlx::PgPool;

use crate::db::models::Submission;
use crate::db::types::SubmissionStatus;

use super::types::{TeacherSubmissionDetails, COLUMNS};

pub(crate) async fn find_by_id(
    pool: &PgPool,
    course_id: &str,
    id: &str,
) -> Result<Option<Submission>, sqlx::Error> {
    sqlx::query_as::<_, Submission>(&format!(
        "SELECT {COLUMNS}
         FROM submissions
         WHERE course_id = $1 AND id = $2"
    ))
    .bind(course_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn find_by_session(
    pool: &PgPool,
    course_id: &str,
    session_id: &str,
) -> Result<Option<Submission>, sqlx::Error> {
    sqlx::query_as::<_, Submission>(&format!(
        "SELECT {COLUMNS}
         FROM submissions
         WHERE course_id = $1 AND session_id = $2"
    ))
    .bind(course_id)
    .bind(session_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn find_teacher_details(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<Option<TeacherSubmissionDetails>, sqlx::Error> {
    sqlx::query_as::<_, TeacherSubmissionDetails>(
        "SELECT s.id,
                s.course_id,
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
                es.variant_assignments,
                u.full_name AS student_name,
                u.username AS student_username
         FROM submissions s
         JOIN exam_sessions es ON es.course_id = s.course_id AND es.id = s.session_id
         JOIN exams e ON e.course_id = es.course_id AND e.id = es.exam_id
         JOIN users u ON u.id = s.student_id
         WHERE s.course_id = $1
           AND s.id = $2",
    )
    .bind(course_id)
    .bind(submission_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn list_by_sessions(
    pool: &PgPool,
    course_id: &str,
    session_ids: &[String],
) -> Result<Vec<Submission>, sqlx::Error> {
    if session_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, Submission>(&format!(
        "SELECT {COLUMNS}
         FROM submissions
         WHERE course_id = $1
           AND session_id = ANY($2)"
    ))
    .bind(course_id)
    .bind(session_ids)
    .fetch_all(pool)
    .await
}

pub(crate) async fn fetch_one_by_id(
    pool: &PgPool,
    course_id: &str,
    id: &str,
) -> Result<Submission, sqlx::Error> {
    sqlx::query_as::<_, Submission>(&format!(
        "SELECT {COLUMNS}
         FROM submissions
         WHERE course_id = $1 AND id = $2"
    ))
    .bind(course_id)
    .bind(id)
    .fetch_one(pool)
    .await
}

pub(crate) async fn find_id_by_session(
    pool: &PgPool,
    course_id: &str,
    session_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT id
         FROM submissions
         WHERE course_id = $1 AND session_id = $2",
    )
    .bind(course_id)
    .bind(session_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn list_flagged_for_retry(
    pool: &PgPool,
    max_retry_count: i32,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT id, course_id
         FROM submissions
         WHERE status = $1
           AND COALESCE(ai_retry_count, 0) < $2
           AND ai_error IS NOT NULL",
    )
    .bind(SubmissionStatus::Flagged)
    .bind(max_retry_count)
    .fetch_all(pool)
    .await
}
