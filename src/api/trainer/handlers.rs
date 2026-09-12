use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use std::time::Duration;
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

#[derive(Debug, Serialize)]
struct StudioTrainerGenerationRequest {
    assistant_id: String,
    topic: String,
    difficulty: String,
    count: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct GenerateRequest {
    #[serde(flatten)]
    payload: TrainerGenerateRequest,
    trainer_id: Option<String>,
    section_id: Option<String>,
}

pub(super) async fn generate_set(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(request): Json<GenerateRequest>,
) -> Result<(StatusCode, Json<TrainerSetResponse>), ApiError> {
    let access = require_course_role(&state, &user, &course_id, CourseRole::Student).await?;
    let payload = request.payload;
    payload.validate().map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let source = resolve_source(state.db(), &course_id, &payload.source).await?;
    if source.code == repositories::trainer_sets::PHYSICAL_CHEMISTRY_SOURCE
        && !access.roles.contains(&CourseRole::Teacher)
    {
        require_generation_unlock(
            &state,
            &course_id,
            &user.id,
            &source.id,
            request.trainer_id.as_deref(),
            request.section_id.as_deref(),
            payload.filters.topic.as_deref(),
        )
        .await?;
    }
    if source.code == "studio_fizicheskaya_himiya"
        && !state.settings().studio_integration().api_url.trim().is_empty()
    {
        return generate_physical_chemistry_set(
            &state,
            &course_id,
            &user.id,
            &payload,
            request.trainer_id.as_deref(),
            request.section_id.as_deref(),
        )
        .await;
    }
    let filter_params = build_filter_params(
        &source.id,
        &payload.filters.paragraph,
        &payload.filters.topic,
        payload.filters.has_answer,
        payload.filters.clone(),
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
        "q": payload.filters.q,
        "task_type": payload.filters.task_type,
        "difficulty": payload.filters.difficulty,
        "volume": payload.filters.volume,
        "has_solution": payload.filters.has_solution,
        "mode": "generated",
        "paragraph": normalize_optional(&payload.filters.paragraph),
        "topic": normalize_optional(&payload.filters.topic),
        "has_answer": payload.filters.has_answer,
        "count": payload.count,
        "seed": payload.seed,
        "trainer_id": request.trainer_id,
        "section_id": request.section_id,
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

async fn require_generation_unlock(
    state: &AppState,
    course_id: &str,
    student_id: &str,
    source_id: &str,
    trainer_id: Option<&str>,
    section_id: Option<&str>,
    topic: Option<&str>,
) -> Result<(), ApiError> {
    let (Some(trainer_id), Some(section_id)) = (trainer_id, section_id) else {
        return Err(ApiError::BadRequest("Выберите тренажёр и подтему".into()));
    };
    // Resolve against the published catalog, never trust the client topic alone.
    let section = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT section FROM course_trainers t,
         LATERAL jsonb_array_elements(t.published->'sections') section
         WHERE t.course_id=$1 AND t.id=$2 AND section->>'id'=$3
           AND EXISTS (SELECT 1 FROM jsonb_array_elements(section->'items') item
             JOIN task_bank_items i ON i.id=item->>'task_id' WHERE i.source_id=$4)",
    )
    .bind(course_id)
    .bind(trainer_id)
    .bind(section_id)
    .bind(source_id)
    .fetch_optional(state.db())
    .await
    .map_err(|e| ApiError::internal(e, "Не удалось загрузить подтему"))?
    .ok_or_else(|| ApiError::BadRequest("Подтема не опубликована или не найдена".into()))?;
    if topic.map(str::trim) != section["title"].as_str().map(str::trim) {
        return Err(ApiError::BadRequest(
            "Тема генерации не соответствует выбранной подтеме".into(),
        ));
    }
    let solved = repositories::trainer_sets::generation_solved_count(
        state.db(),
        course_id,
        student_id,
        trainer_id,
        section_id,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Не удалось загрузить прогресс"))?;
    if solved < 3 {
        return Err(ApiError::Forbidden("решите 3 задачи"));
    }
    Ok(())
}

async fn generate_physical_chemistry_set(
    state: &AppState,
    course_id: &str,
    student_id: &str,
    payload: &TrainerGenerateRequest,
    trainer_id: Option<&str>,
    section_id: Option<&str>,
) -> Result<(StatusCode, Json<TrainerSetResponse>), ApiError> {
    let topic = payload
        .filters
        .topic
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
        ApiError::BadRequest("Для генерации выберите подтему физической химии".into())
    })?;
    let difficulty = payload.filters.difficulty.as_deref().unwrap_or("easy").trim().to_string();
    let assistant = repositories::course_ai_assistants::find(state.db(), course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Не удалось загрузить ассистента курса"))?
        .ok_or_else(|| {
            ApiError::UnprocessableEntity("Ассистент физической химии ещё не опубликован".into())
        })?;
    let settings = state.settings().studio_integration();
    if settings.token.trim().is_empty() {
        return Err(ApiError::ServiceUnavailable(
            "Генерация задач временно недоступна: Studio не настроена".into(),
        ));
    }
    let url = format!("{}/api/internal/trainer/generate", settings.api_url.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(&settings.token)
        .json(&StudioTrainerGenerationRequest {
            assistant_id: assistant.studio_assistant_id,
            topic: topic.to_string(),
            difficulty: difficulty.clone(),
            count: payload.count,
        })
        .timeout(Duration::from_secs(1200))
        .send()
        .await
        .map_err(|e| ApiError::ServiceUnavailable(format!("Генерация задач не завершена: {e}")))?;
    if !response.status().is_success() {
        return Err(ApiError::ServiceUnavailable(format!(
            "Генерация задач не завершена: Studio вернула HTTP {}",
            response.status()
        )));
    }
    let export = response
        .json::<crate::api::studio_grading::TaskBankExport>()
        .await
        .map_err(|e| ApiError::internal(e, "Studio вернула некорректный набор задач"))?;
    let imported = crate::api::studio_grading::import_task_bank(state, course_id, export).await?;
    // Studio exports only verified tasks. Keep a nonempty verified subset;
    // never fill missing slots from the bank or initiate another paid call.
    validate_generated_count(imported.item_ids.len(), payload.count)?;
    let actual_count = imported.item_ids.len() as i64;

    let now = primitive_now_utc();
    let trainer_set_id = Uuid::new_v4().to_string();
    let filters_json = serde_json::json!({
        "q": payload.filters.q,
        "task_type": payload.filters.task_type,
        "difficulty": difficulty,
        "volume": payload.filters.volume,
        "has_solution": true,
        "mode": "studio_generated",
        "trainer_id": trainer_id,
        "section_id": section_id,
        "paragraph": normalize_optional(&payload.filters.paragraph),
        "topic": topic,
        "has_answer": true,
        "count": actual_count,
        "requested_count": payload.count,
        "pending_count": payload.count - actual_count,
    });
    let title = payload
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{} · новый набор", topic));
    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Не удалось начать сохранение набора"))?;
    repositories::trainer_sets::create(
        &mut *tx,
        repositories::trainer_sets::CreateTrainerSet {
            id: &trainer_set_id,
            student_id,
            course_id,
            title: &title,
            source_id: &imported.source_id,
            filters: filters_json,
            now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Не удалось сохранить набор задач"))?;
    repositories::trainer_sets::insert_items(&mut tx, &trainer_set_id, &imported.item_ids)
        .await
        .map_err(|e| ApiError::internal(e, "Не удалось добавить задачи в набор"))?;
    tx.commit()
        .await
        .map_err(|e| ApiError::internal(e, "Не удалось завершить сохранение набора"))?;
    let response = load_set_response(state, course_id, student_id, &trainer_set_id).await?;
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

    let source = resolve_source(state.db(), &course_id, &payload.source).await?;
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
    course_id: &str,
    source_code: &str,
) -> Result<crate::db::models::TaskBankSource, ApiError> {
    let source = repositories::task_bank::find_source_by_code_for_course(
        pool,
        course_id,
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
                // Answers are revealed explicitly inside the practice attempt.
                answer: None,
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

pub(super) fn validate_generated_count(actual: usize, requested: i64) -> Result<(), ApiError> {
    if !usize::try_from(requested).is_ok_and(|requested| (1..=requested).contains(&actual)) {
        return Err(ApiError::ServiceUnavailable(
            "Studio вернула пустой набор или больше задач, чем запрошено".into(),
        ));
    }
    Ok(())
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
    filters: crate::schemas::trainer::TrainerFilters,
) -> repositories::task_bank::FilterParams {
    repositories::task_bank::FilterParams {
        filters,
        source_id: source_id.to_string(),
        paragraph: normalize_optional(paragraph),
        topic: normalize_optional(topic),
        has_answer,
    }
}
