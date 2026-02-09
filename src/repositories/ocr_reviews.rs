use sqlx::PgPool;
use time::PrimitiveDateTime;
use uuid::Uuid;

use crate::db::models::{SubmissionOcrIssue, SubmissionOcrReview};
use crate::db::types::OcrPageStatus;

#[derive(Debug, Clone)]
pub(crate) struct NewOcrIssue {
    pub(crate) anchor: serde_json::Value,
    pub(crate) original_text: Option<String>,
    pub(crate) suggested_text: Option<String>,
    pub(crate) note: String,
    pub(crate) severity: crate::db::types::OcrIssueSeverity,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct OcrReviewCompletionStats {
    pub(crate) total_pages: i64,
    pub(crate) reviewed_pages: i64,
    pub(crate) total_issues: i64,
    pub(crate) reported_pages: i64,
}

pub(crate) async fn list_reviews_by_submission(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<Vec<SubmissionOcrReview>, sqlx::Error> {
    sqlx::query_as::<_, SubmissionOcrReview>(
        "SELECT id, course_id, submission_id, image_id, student_id, page_status, issue_count, created_at, updated_at
         FROM submission_ocr_reviews
         WHERE course_id = $1
           AND submission_id = $2",
    )
    .bind(course_id)
    .bind(submission_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_issues_by_submission(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<Vec<SubmissionOcrIssue>, sqlx::Error> {
    sqlx::query_as::<_, SubmissionOcrIssue>(
        "SELECT id, course_id, ocr_review_id, submission_id, image_id, anchor, original_text, suggested_text,
                note, severity, created_at, updated_at
         FROM submission_ocr_issues
         WHERE course_id = $1
           AND submission_id = $2
         ORDER BY created_at ASC",
    )
    .bind(course_id)
    .bind(submission_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn upsert_page_review(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
    image_id: &str,
    student_id: &str,
    page_status: OcrPageStatus,
    issues: &[NewOcrIssue],
    now: PrimitiveDateTime,
) -> Result<String, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let review_id = Uuid::new_v4().to_string();

    let review_id = sqlx::query_scalar::<_, String>(
        "INSERT INTO submission_ocr_reviews (
            id, course_id, submission_id, image_id, student_id, page_status, issue_count, created_at, updated_at
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$8)
         ON CONFLICT (submission_id, image_id) DO UPDATE
             SET student_id = EXCLUDED.student_id,
                 page_status = EXCLUDED.page_status,
                 updated_at = EXCLUDED.updated_at
         RETURNING id",
    )
    .bind(&review_id)
    .bind(course_id)
    .bind(submission_id)
    .bind(image_id)
    .bind(student_id)
    .bind(page_status)
    .bind(issues.len() as i32)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "DELETE FROM submission_ocr_issues
         WHERE course_id = $1
           AND ocr_review_id = $2",
    )
    .bind(course_id)
    .bind(&review_id)
    .execute(&mut *tx)
    .await?;

    for issue in issues {
        let issue_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO submission_ocr_issues (
                id, course_id, ocr_review_id, submission_id, image_id, anchor, original_text,
                suggested_text, note, severity, created_at, updated_at
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$11)",
        )
        .bind(issue_id)
        .bind(course_id)
        .bind(&review_id)
        .bind(submission_id)
        .bind(image_id)
        .bind(&issue.anchor)
        .bind(&issue.original_text)
        .bind(&issue.suggested_text)
        .bind(&issue.note)
        .bind(issue.severity)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "UPDATE submission_ocr_reviews
         SET issue_count = $1,
             updated_at = $2
         WHERE id = $3
           AND course_id = $4",
    )
    .bind(issues.len() as i32)
    .bind(now)
    .bind(&review_id)
    .bind(course_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(review_id)
}

pub(crate) async fn clear_reviews_by_submission(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM submission_ocr_reviews
         WHERE course_id = $1
           AND submission_id = $2",
    )
    .bind(course_id)
    .bind(submission_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn review_completion_stats(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
    student_id: &str,
) -> Result<OcrReviewCompletionStats, sqlx::Error> {
    sqlx::query_as::<_, OcrReviewCompletionStats>(
        "SELECT
            COALESCE((
                SELECT COUNT(*)
                FROM submission_images si
                WHERE si.course_id = $1
                  AND si.submission_id = $2
            ), 0) AS total_pages,
            COALESCE((
                SELECT COUNT(*)
                FROM submission_ocr_reviews sor
                WHERE sor.course_id = $1
                  AND sor.submission_id = $2
                  AND sor.student_id = $3
            ), 0) AS reviewed_pages,
            COALESCE((
                SELECT SUM(sor.issue_count)::BIGINT
                FROM submission_ocr_reviews sor
                WHERE sor.course_id = $1
                  AND sor.submission_id = $2
                  AND sor.student_id = $3
            ), 0) AS total_issues,
            COALESCE((
                SELECT COUNT(*)
                FROM submission_ocr_reviews sor
                WHERE sor.course_id = $1
                  AND sor.submission_id = $2
                  AND sor.student_id = $3
                  AND sor.page_status = $4
            ), 0) AS reported_pages",
    )
    .bind(course_id)
    .bind(submission_id)
    .bind(student_id)
    .bind(OcrPageStatus::Reported)
    .fetch_one(pool)
    .await
}
