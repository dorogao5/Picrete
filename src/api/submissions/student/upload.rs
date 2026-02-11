use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    Json,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_role, CurrentUser};
use crate::api::validation::validate_image_upload;
use crate::core::state::AppState;
use crate::db::types::{CourseRole, SessionStatus};
use crate::services::submission_images::SubmissionImagesService;

pub(in crate::api::submissions) async fn presigned_upload_url(
    Path((course_id, session_id)): Path<(String, String)>,
    Query(query): Query<crate::api::submissions::PresignQuery>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;
    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status, _) =
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
    let key = format!("submissions/{course_id}/{session_id}/{object_id}_{filename}");
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
    Path((course_id, session_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;
    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (hard_deadline, session_status, _) =
        crate::api::submissions::helpers::enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::BadRequest("Session is not active".to_string()));
    }
    if OffsetDateTime::now_utc().unix_timestamp() >= hard_deadline.assume_utc().unix_timestamp() {
        return Err(ApiError::BadRequest("Session has expired".to_string()));
    }

    let mut file_bytes: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;
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
        }
        // Ignore legacy client-provided order_index, server assigns order.
    }

    let file_bytes =
        file_bytes.ok_or_else(|| ApiError::BadRequest("File is required".to_string()))?;
    let filename = filename.unwrap_or_else(|| "image.jpg".to_string());
    let content_type = content_type.unwrap_or_else(|| "application/octet-stream".to_string());

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

    let image = SubmissionImagesService::upload_from_web(
        &state,
        &session,
        &filename,
        &content_type,
        file_bytes,
    )
    .await
    .map_err(map_upload_service_error)?;

    Ok(Json(serde_json::json!({
        "message": "File uploaded successfully",
        "image_id": image.id,
        "image": {
            "id": image.id,
            "filename": image.filename,
            "mime_type": image.mime_type,
            "file_size": image.file_size,
            "order_index": image.order_index,
            "upload_source": image.upload_source,
            "uploaded_at": image.uploaded_at,
            "view_url": image.view_url,
        }
    })))
}

pub(in crate::api::submissions) async fn list_session_images(
    Path((course_id, session_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let images = SubmissionImagesService::list_for_session(&state, &session)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to list session images"))?;

    Ok(Json(serde_json::json!({
        "items": images.into_iter().map(|image| serde_json::json!({
            "id": image.id,
            "filename": image.filename,
            "mime_type": image.mime_type,
            "file_size": image.file_size,
            "order_index": image.order_index,
            "upload_source": image.upload_source,
            "uploaded_at": image.uploaded_at,
            "view_url": image.view_url,
        })).collect::<Vec<_>>()
    })))
}

pub(in crate::api::submissions) async fn delete_session_image(
    Path((course_id, session_id, image_id)): Path<(String, String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let session =
        crate::api::submissions::helpers::fetch_session(state.db(), &course_id, &session_id)
            .await?;
    if session.student_id != user.id {
        return Err(ApiError::Forbidden("Access denied"));
    }

    let (_, session_status, _) =
        crate::api::submissions::helpers::enforce_deadline(&session, state.db()).await?;
    if session_status != SessionStatus::Active {
        return Err(ApiError::Conflict("Cannot delete images for inactive session".to_string()));
    }

    let deleted = SubmissionImagesService::delete_for_session(&state, &session, &image_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to delete session image"))?;

    if !deleted {
        return Err(ApiError::NotFound("Image not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn map_upload_service_error(error: anyhow::Error) -> ApiError {
    let text = error.to_string();

    if text.contains("Maximum number of images per submission exceeded") {
        return ApiError::BadRequest(text);
    }

    if text.contains("S3 storage is not configured") {
        return ApiError::ServiceUnavailable(text);
    }

    ApiError::internal(error, "Failed to upload image")
}
