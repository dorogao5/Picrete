use sqlx::PgPool;

use crate::db::models::SubmissionImage;
use crate::db::types::UploadSource;

pub(crate) const COLUMNS: &str = "\
    id, course_id, submission_id, filename, file_path, file_size, mime_type, \
    is_processed, ocr_status, ocr_text, ocr_markdown, ocr_chunks, ocr_model, \
    ocr_completed_at, ocr_error, ocr_request_id, quality_score, order_index, upload_source, \
    perceptual_hash, uploaded_at, processed_at";

pub(crate) async fn find_by_id(
    pool: &PgPool,
    course_id: &str,
    id: &str,
) -> Result<Option<SubmissionImage>, sqlx::Error> {
    sqlx::query_as::<_, SubmissionImage>(&format!(
        "SELECT {COLUMNS}
         FROM submission_images
         WHERE course_id = $1 AND id = $2"
    ))
    .bind(course_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn list_by_submission(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<Vec<SubmissionImage>, sqlx::Error> {
    sqlx::query_as::<_, SubmissionImage>(&format!(
        "SELECT {COLUMNS}
         FROM submission_images
         WHERE course_id = $1 AND submission_id = $2
         ORDER BY order_index"
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
) -> Result<Vec<SubmissionImage>, sqlx::Error> {
    if submission_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, SubmissionImage>(&format!(
        "SELECT {COLUMNS}
         FROM submission_images
         WHERE course_id = $1 AND submission_id = ANY($2)
         ORDER BY order_index"
    ))
    .bind(course_id)
    .bind(submission_ids)
    .fetch_all(pool)
    .await
}

pub(crate) async fn count_by_submission_with_executor(
    executor: impl sqlx::PgExecutor<'_>,
    course_id: &str,
    submission_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM submission_images
         WHERE course_id = $1 AND submission_id = $2",
    )
    .bind(course_id)
    .bind(submission_id)
    .fetch_one(executor)
    .await
}

pub(crate) async fn next_order_index_for_submission(
    executor: impl sqlx::PgExecutor<'_>,
    course_id: &str,
    submission_id: &str,
) -> Result<i32, sqlx::Error> {
    let next = sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(MAX(order_index), -1) + 1
         FROM submission_images
         WHERE course_id = $1 AND submission_id = $2",
    )
    .bind(course_id)
    .bind(submission_id)
    .fetch_one(executor)
    .await?;

    Ok(next)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_with_executor(
    executor: impl sqlx::PgExecutor<'_>,
    id: &str,
    course_id: &str,
    submission_id: &str,
    filename: &str,
    file_path: &str,
    file_size: i64,
    mime_type: &str,
    order_index: i32,
    upload_source: UploadSource,
    uploaded_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO submission_images (
            id, course_id, submission_id, filename, file_path, file_size, mime_type,
            order_index, upload_source, is_processed, uploaded_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id)
    .bind(course_id)
    .bind(submission_id)
    .bind(filename)
    .bind(file_path)
    .bind(file_size)
    .bind(mime_type)
    .bind(order_index)
    .bind(upload_source)
    .bind(false)
    .bind(uploaded_at)
    .execute(executor)
    .await?;
    Ok(())
}

pub(crate) async fn delete_by_submission_and_id(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
    id: &str,
) -> Result<Option<SubmissionImage>, sqlx::Error> {
    sqlx::query_as::<_, SubmissionImage>(&format!(
        "DELETE FROM submission_images
         WHERE course_id = $1
           AND submission_id = $2
           AND id = $3
         RETURNING {COLUMNS}"
    ))
    .bind(course_id)
    .bind(submission_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn mark_ocr_processing(
    pool: &PgPool,
    id: &str,
    request_id: Option<&str>,
    started_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submission_images
         SET ocr_status = $1,
             ocr_request_id = $2,
             ocr_error = NULL,
             processed_at = $3
         WHERE id = $4",
    )
    .bind(crate::db::types::OcrImageStatus::Processing)
    .bind(request_id)
    .bind(started_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn mark_ocr_ready(
    pool: &PgPool,
    id: &str,
    ocr_text: Option<&str>,
    ocr_markdown: Option<&str>,
    ocr_chunks: Option<&serde_json::Value>,
    ocr_model: Option<&str>,
    completed_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submission_images
         SET ocr_status = $1,
             ocr_text = $2,
             ocr_markdown = $3,
             ocr_chunks = $4,
             ocr_model = $5,
             ocr_completed_at = $6,
             ocr_error = NULL,
             ocr_request_id = NULL,
             processed_at = $6,
             is_processed = TRUE
         WHERE id = $7",
    )
    .bind(crate::db::types::OcrImageStatus::Ready)
    .bind(ocr_text)
    .bind(ocr_markdown)
    .bind(ocr_chunks)
    .bind(ocr_model)
    .bind(completed_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn mark_ocr_failed(
    pool: &PgPool,
    id: &str,
    error: &str,
    updated_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submission_images
         SET ocr_status = $1,
             ocr_error = $2,
             ocr_request_id = NULL,
             processed_at = $3
         WHERE id = $4",
    )
    .bind(crate::db::types::OcrImageStatus::Failed)
    .bind(error)
    .bind(updated_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn mark_stale_processing_failed_by_submission(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
    error: &str,
    updated_at: time::PrimitiveDateTime,
) -> Result<u64, sqlx::Error> {
    let updated = sqlx::query(
        "UPDATE submission_images
         SET ocr_status = $1,
             ocr_error = $2,
             ocr_request_id = NULL,
             processed_at = $3
         WHERE course_id = $4
           AND submission_id = $5
           AND ocr_status = $6",
    )
    .bind(crate::db::types::OcrImageStatus::Failed)
    .bind(error)
    .bind(updated_at)
    .bind(course_id)
    .bind(submission_id)
    .bind(crate::db::types::OcrImageStatus::Processing)
    .execute(pool)
    .await?;

    Ok(updated.rows_affected())
}

pub(crate) async fn reset_ocr_by_submission(
    pool: &PgPool,
    course_id: &str,
    submission_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE submission_images
         SET ocr_status = $1,
             ocr_text = NULL,
             ocr_markdown = NULL,
             ocr_chunks = NULL,
             ocr_model = NULL,
             ocr_completed_at = NULL,
             ocr_error = NULL,
             ocr_request_id = NULL,
             processed_at = NULL,
             is_processed = FALSE
         WHERE course_id = $2 AND submission_id = $3",
    )
    .bind(crate::db::types::OcrImageStatus::Pending)
    .bind(course_id)
    .bind(submission_id)
    .execute(pool)
    .await?;
    Ok(())
}
