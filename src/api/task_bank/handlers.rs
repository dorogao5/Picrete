use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_membership, CurrentUser};
use crate::api::pagination::PaginatedResponse;
use crate::core::state::AppState;
use crate::repositories;
use crate::schemas::task_bank::{
    TaskBankItemImageResponse, TaskBankItemResponse, TaskBankSourceResponse,
};
use crate::services::materials::{self, MaterialsError};

#[derive(Debug, Deserialize)]
pub(super) struct ListTaskBankItemsQuery {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    paragraph: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    has_answer: Option<bool>,
    #[serde(default)]
    skip: i64,
    #[serde(default = "crate::api::pagination::default_limit")]
    limit: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct ViewImageQuery {
    #[serde(default)]
    size: Option<String>,
}

pub(super) async fn list_sources(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<TaskBankSourceResponse>>, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;

    let sources = repositories::task_bank::list_sources(state.db())
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load task bank sources"))?;
    let response = sources
        .into_iter()
        .map(|source| TaskBankSourceResponse {
            id: source.id,
            code: source.code,
            title: source.title,
            version: source.version,
        })
        .collect();

    Ok(Json(response))
}

pub(super) async fn list_items(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Query(query): Query<ListTaskBankItemsQuery>,
) -> Result<Json<PaginatedResponse<TaskBankItemResponse>>, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;

    let skip = query.skip.max(0);
    let limit = query.limit.clamp(1, 1000);

    let rows = repositories::task_bank::list_items(
        state.db(),
        repositories::task_bank::ListItemsParams {
            source_code: query
                .source
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty()),
            paragraph: query
                .paragraph
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            topic: query
                .topic
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            has_answer: query.has_answer,
            skip,
            limit,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to list task bank items"))?;

    let item_ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let images = repositories::task_bank::list_item_images_by_item_ids(state.db(), &item_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to list task bank item images"))?;

    let mut images_by_item_id = HashMap::<String, Vec<crate::db::models::TaskBankItemImage>>::new();
    for image in images {
        images_by_item_id.entry(image.task_bank_item_id.clone()).or_default().push(image);
    }

    let api_prefix = state.settings().api().api_v1_str.trim_end_matches('/');
    let total_count = rows.first().map(|row| row.total_count).unwrap_or(0);
    let items = rows
        .into_iter()
        .map(|row| {
            let images = images_by_item_id
                .remove(&row.id)
                .unwrap_or_default()
                .into_iter()
                .map(|image| TaskBankItemImageResponse {
                    id: image.id.clone(),
                    thumbnail_url: format!(
                        "{api_prefix}/courses/{course_id}/task-bank/items/{}/images/{}/view?size=thumbnail",
                        row.id, image.id
                    ),
                    full_url: format!(
                        "{api_prefix}/courses/{course_id}/task-bank/items/{}/images/{}/view?size=full",
                        row.id, image.id
                    ),
                })
                .collect::<Vec<_>>();

            TaskBankItemResponse {
                id: row.id,
                source: row.source_code,
                number: row.number,
                paragraph: row.paragraph,
                topic: row.topic,
                text: row.text,
                has_answer: row.has_answer,
                answer: row.answer,
                images,
            }
        })
        .collect::<Vec<_>>();

    Ok(Json(PaginatedResponse { items, total_count, skip, limit }))
}

pub(super) async fn view_item_image(
    Path((course_id, item_id, image_id)): Path<(String, String, String)>,
    Query(query): Query<ViewImageQuery>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;

    if let Some(size) = query.size {
        if !matches!(size.as_str(), "thumbnail" | "full") {
            return Err(ApiError::BadRequest(
                "size must be either 'thumbnail' or 'full'".to_string(),
            ));
        }
    }

    let image = repositories::task_bank::find_item_image(state.db(), &item_id, &image_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load task bank image"))?
        .ok_or_else(|| ApiError::NotFound("Task bank image not found".to_string()))?;

    let path = materials::resolve_task_bank_media_path(state.settings(), &image.relative_path)
        .map_err(map_materials_error)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to read image file"))?;

    let mut response = (StatusCode::OK, bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&image.mime_type)
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
            metrics::counter!("media_access_denied_total", "kind" => "task_bank_image".to_string())
                .increment(1);
            ApiError::Forbidden("Path is not allowed")
        }
        MaterialsError::NotFound => ApiError::NotFound("File not found".to_string()),
        MaterialsError::Io(err) => ApiError::internal(err, "File access failed"),
    }
}
