use axum::{
    extract::{Multipart, Path, Query, State},
    Json,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::CurrentUser;
use crate::api::validation::validate_image_upload;
use crate::core::state::AppState;
use crate::db::types::SessionStatus;
use crate::repositories;

pub(in crate::api::submissions) async fn presigned_upload_url(
    Path(session_id): Path<String>,
    Query(query): Query<crate::api::submissions::PresignQuery>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = crate::api::submissions::helpers::fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) =
        crate::api::submissions::helpers::enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::BadRequest("Session is not active".to_string()));
    }

    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp() {
        return Err(ApiError::BadRequest("Session has expired".to_string()));
    }

    validate_image_upload(
        &query.filename,
        &query.content_type,
        &state.settings().storage().allowed_image_extensions,
    )?;

    let storage = state.storage().ok_or_else(|| {
        ApiError::ServiceUnavailable(
            "Direct upload not available. Use standard upload endpoint.".to_string(),
        )
    })?;

    let filename = crate::api::submissions::helpers::sanitized_filename(&query.filename);
    let object_id = Uuid::new_v4().to_string();
    let key = format!("submissions/{session_id}/{object_id}_{filename}");
    let expires =
        std::time::Duration::from_secs(state.settings().exam().presigned_url_expire_minutes * 60);
    let presigned = storage
        .presign_put(&key, &query.content_type, expires)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to generate upload URL"))?;

    Ok(Json(serde_json::json!({
        "upload_url": presigned,
        "s3_key": key,
        "method": "PUT",
        "headers": {"Content-Type": query.content_type}
    })))
}

pub(in crate::api::submissions) async fn upload_image(
    Path(session_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session = crate::api::submissions::helpers::fetch_session(state.db(), &session_id).await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status) =
        crate::api::submissions::helpers::enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::BadRequest("Session is not active".to_string()));
    }
    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp() {
        return Err(ApiError::BadRequest("Session has expired".to_string()));
    }

    let storage = state.storage().ok_or_else(|| {
        ApiError::ServiceUnavailable(
            "S3 storage is not configured. Please configure Yandex Object Storage.".to_string(),
        )
    })?;
    let submission_id =
        crate::api::submissions::helpers::ensure_submission(state.db(), &session).await?;

    let current_images = repositories::images::count_by_submission(state.db(), &submission_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to count submission images"))?;

    let max_images = state.settings().storage().max_images_per_submission as i64;
    if current_images >= max_images {
        return Err(ApiError::BadRequest(format!(
            "Maximum number of images per submission exceeded ({max_images})"
        )));
    }

    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut order_index: Option<i32> = None;
    let max_bytes = state.settings().storage().max_upload_size_mb * 1024 * 1024;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::BadRequest("Invalid multipart data".to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            filename = field.file_name().map(|s| s.to_string());
            content_type = field.content_type().map(|s| s.to_string());
            let mut bytes = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| ApiError::BadRequest("Failed to read file".to_string()))?
            {
                let next_size = bytes.len() as u64 + chunk.len() as u64;
                if next_size > max_bytes {
                    return Err(ApiError::BadRequest(format!(
                        "File size exceeds {}MB limit",
                        state.settings().storage().max_upload_size_mb
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            file_bytes = Some(bytes);
        } else if name == "order_index" {
            let text = field
                .text()
                .await
                .map_err(|_| ApiError::BadRequest("Invalid order index".to_string()))?;
            order_index = Some(text.parse::<i32>().map_err(|_| {
                ApiError::BadRequest("order_index must be a valid integer".to_string())
            })?);
        }
    }

    let file_bytes =
        file_bytes.ok_or_else(|| ApiError::BadRequest("File is required".to_string()))?;
    let filename = filename.unwrap_or_else(|| "image.jpg".to_string());
    let content_type = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let order_index =
        order_index.ok_or_else(|| ApiError::BadRequest("order_index is required".to_string()))?;

    validate_image_upload(
        &filename,
        &content_type,
        &state.settings().storage().allowed_image_extensions,
    )?;

    if file_bytes.len() as u64 > max_bytes {
        return Err(ApiError::BadRequest(format!(
            "File size exceeds {}MB limit",
            state.settings().storage().max_upload_size_mb
        )));
    }

    let image_id = Uuid::new_v4().to_string();
    let key = format!(
        "submissions/{}/{}_{}",
        session_id,
        image_id,
        crate::api::submissions::helpers::sanitized_filename(&filename)
    );

    let (file_size, _hash) = storage
        .upload_bytes(&key, &content_type, file_bytes)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to upload file to S3"))?;

    repositories::images::insert(
        state.db(),
        &image_id,
        &submission_id,
        &filename,
        &key,
        file_size,
        &content_type,
        order_index,
        crate::api::submissions::helpers::now_primitive(),
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to store image metadata"))?;

    Ok(Json(serde_json::json!({
        "message": "File uploaded successfully",
        "image_id": image_id
    })))
}
