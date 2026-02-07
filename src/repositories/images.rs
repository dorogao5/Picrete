use sqlx::PgPool;

use crate::db::models::SubmissionImage;

pub(crate) const COLUMNS: &str = "\
    id, submission_id, filename, file_path, file_size, mime_type, \
    is_processed, ocr_text, quality_score, order_index, perceptual_hash, \
    uploaded_at, processed_at";

pub(crate) async fn find_by_id(
    pool: &PgPool,
    id: &str,
) -> Result<Option<SubmissionImage>, sqlx::Error> {
    sqlx::query_as::<_, SubmissionImage>(&format!(
        "SELECT {COLUMNS} FROM submission_images WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn list_by_submission(
    pool: &PgPool,
    submission_id: &str,
) -> Result<Vec<SubmissionImage>, sqlx::Error> {
    sqlx::query_as::<_, SubmissionImage>(&format!(
        "SELECT {COLUMNS} FROM submission_images WHERE submission_id = $1 ORDER BY order_index"
    ))
    .bind(submission_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn count_by_submission(
    pool: &PgPool,
    submission_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM submission_images WHERE submission_id = $1")
        .bind(submission_id)
        .fetch_one(pool)
        .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert(
    pool: &PgPool,
    id: &str,
    submission_id: &str,
    filename: &str,
    file_path: &str,
    file_size: i64,
    mime_type: &str,
    order_index: i32,
    uploaded_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO submission_images (
            id, submission_id, filename, file_path, file_size, mime_type,
            order_index, is_processed, uploaded_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(id)
    .bind(submission_id)
    .bind(filename)
    .bind(file_path)
    .bind(file_size)
    .bind(mime_type)
    .bind(order_index)
    .bind(false)
    .bind(uploaded_at)
    .execute(pool)
    .await?;
    Ok(())
}
