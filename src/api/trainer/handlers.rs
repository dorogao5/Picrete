use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_role, CurrentUser};
use crate::api::pagination::PaginatedResponse;
use crate::core::state::AppState;
use crate::core::time::{format_primitive, primitive_now_utc};
use crate::db::types::CourseRole;
use crate::repositories;
use crate::schemas::trainer::{
    TrainerGenerateRequest, TrainerManualCreateRequest, TrainerSetItemImageResponse,
    TrainerSetItemResponse, TrainerSetResponse, TrainerSetSummaryResponse,
};

const MAX_GENERATION_CANDIDATES: i64 = 20_000;

#[derive(Debug, Deserialize)]
pub(super) struct ListTrainerSetsQuery {
    #[serde(default)]
    skip: i64,
    #[serde(default = "crate::api::pagination::default_limit")]
    limit: i64,
}

pub(super) async fn generate_set(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(payload): Json<TrainerGenerateRequest>,
) -> Result<(StatusCode, Json<TrainerSetResponse>), ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;
    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let source = resolve_source(state.db(), &payload.source).await?;
    let filter_params = build_filter_params(
        &source.id,
        &payload.filters.paragraph,
        &payload.filters.topic,
        payload.filters.has_answer,
    );
    let total_candidates =
        repositories::task_bank::count_items_by_filters(state.db(), &filter_params)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to count task bank items"))?;

    if total_candidates < payload.count {
        return Err(ApiError::UnprocessableEntity(format!(
            "Requested {} items but only {} match the selected filters",
            payload.count, total_candidates
        )));
    }
    if total_candidates > MAX_GENERATION_CANDIDATES {
        return Err(ApiError::BadRequest(format!(
            "Filter is too broad ({} items). Narrow filters and try again",
            total_candidates
        )));
    }

    let mut candidate_ids = repositories::task_bank::list_item_ids_by_filters(
        state.db(),
        &filter_params,
        MAX_GENERATION_CANDIDATES,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to list task bank item ids"))?;
    if candidate_ids.len() < payload.count as usize {
        return Err(ApiError::Internal(
            "Task bank candidate selection is inconsistent".to_string(),
        ));
    }

    if let Some(seed) = payload.seed {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        candidate_ids.shuffle(&mut rng);
    } else {
        let mut rng = rand::thread_rng();
        candidate_ids.shuffle(&mut rng);
    }
    candidate_ids.truncate(payload.count as usize);

    let now = primitive_now_utc();
    let trainer_set_id = Uuid::new_v4().to_string();
    let filters_json = serde_json::json!({
        "mode": "generated",
        "paragraph": normalize_optional(&payload.filters.paragraph),
        "topic": normalize_optional(&payload.filters.topic),
        "has_answer": payload.filters.has_answer,
        "count": payload.count,
        "seed": payload.seed,
    });
    let title = payload
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Тренировка {}", now.date()));

    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start trainer transaction"))?;
    repositories::trainer_sets::create(
        &mut *tx,
        repositories::trainer_sets::CreateTrainerSet {
            id: &trainer_set_id,
            student_id: &user.id,
            course_id: &course_id,
            title: &title,
            source_id: &source.id,
            filters: filters_json,
            now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create trainer set"))?;
    repositories::trainer_sets::insert_items(&mut tx, &trainer_set_id, &candidate_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to add trainer set items"))?;
    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit trainer set"))?;

    let response = load_set_response(&state, &course_id, &user.id, &trainer_set_id).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

pub(super) async fn create_manual_set(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(payload): Json<TrainerManualCreateRequest>,
) -> Result<(StatusCode, Json<TrainerSetResponse>), ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;
    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let source = resolve_source(state.db(), &payload.source).await?;
    let numbers = normalize_numbers(&payload.numbers);
    if numbers.is_empty() {
        return Err(ApiError::BadRequest("numbers must not be empty".to_string()));
    }

    let items = repositories::task_bank::list_items_by_numbers(state.db(), &source.id, &numbers)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to fetch task bank items by numbers"))?;
    let item_numbers = items.iter().map(|item| item.number.clone()).collect::<HashSet<_>>();
    let invalid_numbers = numbers
        .iter()
        .filter(|number| !item_numbers.contains(number.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !invalid_numbers.is_empty() {
        return Err(ApiError::UnprocessableEntity(format!(
            "Unknown task numbers: {}",
            invalid_numbers.join(", ")
        )));
    }

    let item_ids_by_number =
        items.into_iter().map(|item| (item.number, item.id)).collect::<HashMap<_, _>>();
    let ordered_item_ids = numbers
        .iter()
        .filter_map(|number| item_ids_by_number.get(number).cloned())
        .collect::<Vec<_>>();

    let now = primitive_now_utc();
    let trainer_set_id = Uuid::new_v4().to_string();
    let filters_json = serde_json::json!({
        "mode": "manual",
        "numbers": numbers,
    });
    let title = payload
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "Выбранные задачи".to_string());

    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to start trainer transaction"))?;
    repositories::trainer_sets::create(
        &mut *tx,
        repositories::trainer_sets::CreateTrainerSet {
            id: &trainer_set_id,
            student_id: &user.id,
            course_id: &course_id,
            title: &title,
            source_id: &source.id,
            filters: filters_json,
            now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to create trainer set"))?;
    repositories::trainer_sets::insert_items(&mut tx, &trainer_set_id, &ordered_item_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to add trainer set items"))?;
    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit trainer set"))?;

    let response = load_set_response(&state, &course_id, &user.id, &trainer_set_id).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

pub(super) async fn list_sets(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Query(query): Query<ListTrainerSetsQuery>,
) -> Result<Json<PaginatedResponse<TrainerSetSummaryResponse>>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let skip = query.skip.max(0);
    let limit = query.limit.clamp(1, 1000);
    let rows = repositories::trainer_sets::list_for_student(
        state.db(),
        repositories::trainer_sets::ListTrainerSetsParams {
            course_id: course_id.clone(),
            student_id: user.id.clone(),
            skip,
            limit,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to list trainer sets"))?;

    let total_count = rows.first().map(|row| row.total_count).unwrap_or(0);
    let items = rows
        .into_iter()
        .map(|row| TrainerSetSummaryResponse {
            id: row.id,
            title: row.title,
            source: row.source_code,
            source_title: row.source_title,
            filters: row.filters.0,
            item_count: row.item_count,
            created_at: format_primitive(row.created_at),
            updated_at: format_primitive(row.updated_at),
        })
        .collect::<Vec<_>>();

    Ok(Json(PaginatedResponse { items, total_count, skip, limit }))
}

pub(super) async fn get_set(
    Path((course_id, set_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<TrainerSetResponse>, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;
    let response = load_set_response(&state, &course_id, &user.id, &set_id).await?;
    Ok(Json(response))
}

pub(super) async fn delete_set(
    Path((course_id, set_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    require_course_role(&state, &user, &course_id, CourseRole::Student).await?;

    let deleted = repositories::trainer_sets::soft_delete(
        state.db(),
        &course_id,
        &user.id,
        &set_id,
        primitive_now_utc(),
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to delete trainer set"))?;
    if !deleted {
        return Err(ApiError::NotFound("Trainer set not found".to_string()));
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn resolve_source(
    pool: &sqlx::PgPool,
    source_code: &str,
) -> Result<crate::db::models::TaskBankSource, ApiError> {
    let source = repositories::task_bank::find_source_by_code(
        pool,
        &source_code.trim().to_ascii_lowercase(),
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to load task bank source"))?;
    source.ok_or_else(|| ApiError::NotFound("Task bank source not found".to_string()))
}

async fn load_set_response(
    state: &AppState,
    course_id: &str,
    student_id: &str,
    set_id: &str,
) -> Result<TrainerSetResponse, ApiError> {
    let set =
        repositories::trainer_sets::find_for_student(state.db(), course_id, student_id, set_id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to load trainer set"))?
            .ok_or_else(|| ApiError::NotFound("Trainer set not found".to_string()))?;

    let source = repositories::task_bank::find_source_by_id(state.db(), &set.source_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load task bank source"))?
        .ok_or_else(|| ApiError::Internal("Trainer set source is missing".to_string()))?;

    let item_ids = repositories::trainer_sets::list_item_ids(state.db(), &set.id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load trainer set item ids"))?;
    let items = repositories::task_bank::list_items_with_source_by_ids(state.db(), &item_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load trainer set items"))?;
    let images = repositories::task_bank::list_item_images_by_item_ids(state.db(), &item_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load trainer set images"))?;

    let mut images_by_item = HashMap::<String, Vec<crate::db::models::TaskBankItemImage>>::new();
    for image in images {
        images_by_item.entry(image.task_bank_item_id.clone()).or_default().push(image);
    }

    let api_prefix = state.settings().api().api_v1_str.trim_end_matches('/');
    let item_responses = items
        .into_iter()
        .map(|item| {
            let images = images_by_item
                .remove(&item.id)
                .unwrap_or_default()
                .into_iter()
                .map(|image| TrainerSetItemImageResponse {
                    id: image.id.clone(),
                    thumbnail_url: format!(
                        "{api_prefix}/courses/{course_id}/task-bank/items/{}/images/{}/view?size=thumbnail",
                        item.id, image.id
                    ),
                    full_url: format!(
                        "{api_prefix}/courses/{course_id}/task-bank/items/{}/images/{}/view?size=full",
                        item.id, image.id
                    ),
                })
                .collect::<Vec<_>>();

            TrainerSetItemResponse {
                id: item.id,
                number: item.number,
                paragraph: item.paragraph,
                topic: item.topic,
                text: item.text,
                has_answer: item.has_answer,
                answer: item.answer,
                images,
            }
        })
        .collect::<Vec<_>>();

    Ok(TrainerSetResponse {
        id: set.id,
        title: set.title,
        source: source.code,
        source_title: source.title,
        filters: set.filters.0,
        created_at: format_primitive(set.created_at),
        updated_at: format_primitive(set.updated_at),
        items: item_responses,
    })
}

fn normalize_optional(value: &Option<String>) -> Option<String> {
    value.as_ref().map(|entry| entry.trim().to_string()).filter(|entry| !entry.is_empty())
}

fn normalize_numbers(raw_numbers: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for raw in raw_numbers {
        let number = raw.trim();
        if number.is_empty() {
            continue;
        }
        if seen.insert(number.to_string()) {
            normalized.push(number.to_string());
        }
    }
    normalized
}

fn build_filter_params(
    source_id: &str,
    paragraph: &Option<String>,
    topic: &Option<String>,
    has_answer: Option<bool>,
) -> repositories::task_bank::FilterParams {
    repositories::task_bank::FilterParams {
        source_id: source_id.to_string(),
        paragraph: normalize_optional(paragraph),
        topic: normalize_optional(topic),
        has_answer,
    }
}
