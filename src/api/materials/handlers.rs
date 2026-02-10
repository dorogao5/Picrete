use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_membership, CurrentUser};
use crate::core::state::AppState;
use crate::services::materials::{self, MaterialsError};

pub(super) async fn addition_pdf_url(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;
    let api_prefix = state.settings().api().api_v1_str.trim_end_matches('/');
    let url = format!("{api_prefix}/courses/{course_id}/materials/addition-pdf/view");

    Ok(Json(serde_json::json!({
        "url": url,
        "open_in_new_tab": true
    })))
}

pub(super) async fn view_addition_pdf(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;

    let pdf_path = materials::addition_pdf_path(state.settings()).map_err(map_materials_error)?;
    let bytes = tokio::fs::read(&pdf_path)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to read additional materials pdf"))?;

    let mut response = (StatusCode::OK, bytes).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/pdf"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=3600"));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline; filename=\"addition.pdf\""),
    );
    Ok(response)
}

pub(super) async fn view_task_bank_image(
    Path((course_id, relative_path)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;

    let file_path = materials::resolve_task_bank_media_path(state.settings(), &relative_path)
        .map_err(map_materials_error)?;
    let bytes = tokio::fs::read(&file_path)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to read task bank image file"))?;
    let mime = materials::guess_mime(&file_path);

    let mut response = (StatusCode::OK, bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=86400"));
    response.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("inline"));
    Ok(response)
}

fn map_materials_error(error: MaterialsError) -> ApiError {
    match error {
        MaterialsError::InvalidRelativePath | MaterialsError::PathOutsideRoot => {
            metrics::counter!("media_access_denied_total", "kind" => "materials".to_string())
                .increment(1);
            ApiError::Forbidden("Path is not allowed")
        }
        MaterialsError::NotFound => ApiError::NotFound("File not found".to_string()),
        MaterialsError::Io(err) => ApiError::internal(err, "File access failed"),
    }
}
