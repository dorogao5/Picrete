use anyhow::{anyhow, Context, Result};
use sqlx::Executor;
use uuid::Uuid;

use crate::core::state::AppState;
use crate::core::time::{format_primitive, primitive_now_utc};
use crate::db::models::ExamSession;
use crate::db::types::UploadSource;
use crate::repositories;

#[derive(Debug, Clone)]
pub(crate) struct SessionImageDto {
    pub(crate) id: String,
    pub(crate) filename: String,
    pub(crate) mime_type: String,
    pub(crate) file_size: i64,
    pub(crate) order_index: i32,
    pub(crate) upload_source: UploadSource,
    pub(crate) uploaded_at: String,
    pub(crate) view_url: Option<String>,
}

pub(crate) struct SubmissionImagesService;

impl SubmissionImagesService {
    pub(crate) async fn upload_from_web(
        state: &AppState,
        session: &ExamSession,
        filename: &str,
        mime_type: &str,
        file_bytes: Vec<u8>,
    ) -> Result<SessionImageDto> {
        Self::upload_with_source(state, session, filename, mime_type, file_bytes, UploadSource::Web)
            .await
    }

    pub(crate) async fn upload_from_telegram(
        state: &AppState,
        session: &ExamSession,
        filename: &str,
        mime_type: &str,
        file_bytes: Vec<u8>,
    ) -> Result<SessionImageDto> {
        Self::upload_with_source(
            state,
            session,
            filename,
            mime_type,
            file_bytes,
            UploadSource::Telegram,
        )
        .await
    }

    async fn upload_with_source(
        state: &AppState,
        session: &ExamSession,
        filename: &str,
        mime_type: &str,
        file_bytes: Vec<u8>,
        upload_source: UploadSource,
    ) -> Result<SessionImageDto> {
        let storage = state.storage().ok_or_else(|| anyhow!("S3 storage is not configured"))?;
        let image_id = Uuid::new_v4().to_string();
        let sanitized = sanitized_filename(filename);
        let key =
            format!("submissions/{}/{}/{}_{}", session.course_id, session.id, image_id, sanitized);

        let (file_size, _) = storage
            .upload_bytes(&key, mime_type, file_bytes)
            .await
            .context("Failed to upload image bytes")?;

        let now = primitive_now_utc();

        let mut tx = state.db().begin().await.context("Failed to start upload transaction")?;

        let db_insert = async {
            let submission_id = ensure_submission_locked(&mut tx, session).await?;

            let current_images = repositories::images::count_by_submission_with_executor(
                &mut *tx,
                &session.course_id,
                &submission_id,
            )
            .await
            .context("Failed to count submission images")?;

            let max_images = state.settings().storage().max_images_per_submission as i64;
            if current_images >= max_images {
                return Err(anyhow!(
                    "Maximum number of images per submission exceeded ({max_images})"
                ));
            }

            let order_index = repositories::images::next_order_index_for_submission(
                &mut *tx,
                &session.course_id,
                &submission_id,
            )
            .await
            .context("Failed to compute next image order index")?;

            repositories::images::insert_with_executor(
                &mut *tx,
                &image_id,
                &session.course_id,
                &submission_id,
                filename,
                &key,
                file_size,
                mime_type,
                order_index,
                upload_source,
                now,
            )
            .await
            .context("Failed to insert submission image")?;

            Ok::<i32, anyhow::Error>(order_index)
        }
        .await;

        let order_index = match db_insert {
            Ok(index) => {
                tx.commit().await.context("Failed to commit image upload transaction")?;
                index
            }
            Err(err) => {
                let _ = tx.rollback().await;
                let _ = storage.delete_object(&key).await;
                return Err(err);
            }
        };

        let source_label = match upload_source {
            UploadSource::Web => "web",
            UploadSource::Telegram => "telegram",
        };
        metrics::counter!("uploads_total", "source" => source_label).increment(1);

        let view_url = storage.presign_get(&key, std::time::Duration::from_secs(300)).await.ok();

        Ok(SessionImageDto {
            id: image_id,
            filename: filename.to_string(),
            mime_type: mime_type.to_string(),
            file_size,
            order_index,
            upload_source,
            uploaded_at: format_primitive(now),
            view_url,
        })
    }

    pub(crate) async fn list_for_session(
        state: &AppState,
        session: &ExamSession,
    ) -> Result<Vec<SessionImageDto>> {
        let Some(submission_id) = repositories::submissions::find_id_by_session(
            state.db(),
            &session.course_id,
            &session.id,
        )
        .await
        .context("Failed to find submission by session")?
        else {
            return Ok(Vec::new());
        };

        let images = repositories::images::list_by_submission(
            state.db(),
            &session.course_id,
            &submission_id,
        )
        .await
        .context("Failed to list submission images")?;

        let storage = state.storage();
        let mut result = Vec::with_capacity(images.len());

        for image in images {
            let view_url = match storage {
                Some(storage) if image.file_path.starts_with("submissions/") => storage
                    .presign_get(&image.file_path, std::time::Duration::from_secs(300))
                    .await
                    .ok(),
                _ => None,
            };

            result.push(SessionImageDto {
                id: image.id,
                filename: image.filename,
                mime_type: image.mime_type,
                file_size: image.file_size,
                order_index: image.order_index,
                upload_source: image.upload_source,
                uploaded_at: format_primitive(image.uploaded_at),
                view_url,
            });
        }

        Ok(result)
    }

    pub(crate) async fn delete_for_session(
        state: &AppState,
        session: &ExamSession,
        image_id: &str,
    ) -> Result<bool> {
        let Some(submission_id) = repositories::submissions::find_id_by_session(
            state.db(),
            &session.course_id,
            &session.id,
        )
        .await
        .context("Failed to find submission by session")?
        else {
            return Ok(false);
        };

        let deleted = repositories::images::delete_by_submission_and_id(
            state.db(),
            &session.course_id,
            &submission_id,
            image_id,
        )
        .await
        .context("Failed to delete submission image metadata")?;

        let Some(image) = deleted else {
            return Ok(false);
        };

        if let Some(storage) = state.storage() {
            if image.file_path.starts_with("submissions/") {
                if let Err(error) = storage.delete_object(&image.file_path).await {
                    tracing::error!(
                        error = %error,
                        image_id = %image.id,
                        path = %image.file_path,
                        "Submission image metadata deleted but storage object cleanup failed"
                    );
                }
            }
        }

        Ok(true)
    }
}

async fn ensure_submission_locked(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session: &ExamSession,
) -> Result<String> {
    if let Some(id) = repositories::submissions::find_id_by_session_for_update(
        &mut **tx,
        &session.course_id,
        &session.id,
    )
    .await
    .context("Failed to lock submission row")?
    {
        return Ok(id);
    }

    let max_score: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(max_score), 100)
         FROM task_types
         WHERE course_id = $1 AND exam_id = $2",
    )
    .bind(&session.course_id)
    .bind(&session.exam_id)
    .fetch_one(&mut **tx)
    .await
    .context("Failed to calculate max score for submission")?;

    let now = primitive_now_utc();
    let created_id = Uuid::new_v4().to_string();
    tx.execute(
        sqlx::query(
            "INSERT INTO submissions (
                id, course_id, session_id, student_id, status, max_score, submitted_at, created_at, updated_at
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (session_id) DO NOTHING",
        )
        .bind(&created_id)
        .bind(&session.course_id)
        .bind(&session.id)
        .bind(&session.student_id)
        .bind(crate::db::types::SubmissionStatus::Uploaded)
        .bind(max_score)
        .bind(now)
        .bind(now)
        .bind(now),
    )
    .await
    .context("Failed to create submission row")?;

    repositories::submissions::find_id_by_session_for_update(
        &mut **tx,
        &session.course_id,
        &session.id,
    )
    .await
    .context("Failed to fetch submission row after creation")?
    .ok_or_else(|| anyhow!("Submission missing after creation"))
}

fn sanitized_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
        .collect();

    if sanitized.is_empty() {
        "upload".to_string()
    } else {
        sanitized
    }
}
