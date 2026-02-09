use sqlx::PgPool;

use crate::db::models::SubmissionScore;

pub(crate) const COLUMNS: &str = "\
    id, course_id, submission_id, task_type_id, criterion_name, criterion_description, \
    ai_score, final_score, max_score, ai_comment, teacher_comment, created_at, updated_at";

pub(crate) async fn list_by_submission(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<Vec<SubmissionScore>, sqlx::Error> {
    sqlx::query_as::<_, SubmissionScore>(&format!(
        "SELECT {COLUMNS}
         FROM submission_scores
         WHERE course_id = $1 AND submission_id = $2"
    ))
    .bind(course_id)
    .bind(submission_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_by_submissions(
    pool: &PgPool,
    course_id: &str,
    submission_ids: &[String],
) -> Result<Vec<SubmissionScore>, sqlx::Error> {
    if submission_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, SubmissionScore>(&format!(
        "SELECT {COLUMNS}
         FROM submission_scores
         WHERE course_id = $1 AND submission_id = ANY($2)"
    ))
    .bind(course_id)
    .bind(submission_ids)
    .fetch_all(pool)
    .await
}
