//! Teacher-only Studio bridge. Uses the production grading engine without submissions.
use crate::{
    api::{assistant::authenticate_studio, errors::ApiError},
    core::state::AppState,
    repositories,
    schemas::trainer::TrainerFilters,
    services::ai_grading::{AiGradingService, LlmPrecheckRequest},
};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/courses/:course_id/task-bank", get(bank))
        .route("/courses/:course_id/grading-preview", post(preview))
        .route("/courses/:course_id/task-bank/:item_id/images/:image_id", get(bank_image))
}

#[derive(Deserialize)]
struct BankQuery {
    q: Option<String>,
    skip: Option<i64>,
}

async fn bank(
    Path(course_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<BankQuery>,
) -> Result<Json<Value>, ApiError> {
    authenticate_studio(&state, &headers)?;
    let rows = repositories::task_bank::list_items(
        state.db(),
        repositories::task_bank::ListItemsParams {
            course_id,
            source_code: Some("sviridov".into()),
            paragraph: None,
            topic: None,
            has_answer: None,
            skip: query.skip.unwrap_or(0).max(0),
            limit: 25,
            filters: TrainerFilters { q: query.q, has_solution: Some(true), ..Default::default() },
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to load Studio bank"))?;
    let ids = rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
    let images = repositories::task_bank::list_item_images_by_item_ids(state.db(), &ids)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load bank images"))?;
    let total = rows.first().map(|r| r.total_count).unwrap_or(0);
    let items: Vec<Value> = rows.into_iter().map(|r| json!({"id":r.id,"number":r.number,"topic":r.topic,"text":r.text,"solution":r.solution,"difficulty":r.difficulty,"task_type":r.task_type,"volume":r.volume,"images":images.iter().filter(|i|i.task_bank_item_id==r.id).map(|i|i.id.clone()).collect::<Vec<_>>()})).collect();
    Ok(Json(json!({"items":items,"total":total})))
}

#[derive(Deserialize)]
struct PreviewRequest {
    task_id: String,
    student_text: String,
    /// None means the currently published course version.
    snapshot: Option<Value>,
}

async fn preview(
    Path(course_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PreviewRequest>,
) -> Result<Json<Value>, ApiError> {
    authenticate_studio(&state, &headers)?;
    if body.student_text.trim().is_empty() || body.student_text.chars().count() > 30_000 {
        return Err(ApiError::UnprocessableEntity(
            "Ответ должен содержать от 1 до 30000 символов".into(),
        ));
    }
    let task = repositories::task_bank::list_items_with_source_by_ids_for_course(
        state.db(),
        &course_id,
        &[body.task_id],
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to load preview task"))?
    .into_iter()
    .next()
    .ok_or_else(|| ApiError::NotFound("Задача не найдена в банке этого курса".into()))?;
    let reference = task.solution.as_deref().filter(|s| !s.trim().is_empty()).ok_or_else(|| {
        ApiError::UnprocessableEntity("Для проверки нужно полное эталонное решение".into())
    })?;
    let draft = body.snapshot.is_some();
    let snapshot = match body.snapshot {
        Some(s) if s.to_string().len() <= 1_600_000 => s,
        Some(_) => return Err(ApiError::UnprocessableEntity("Снимок слишком большой".into())),
        None => {
            repositories::course_ai_assistants::find(state.db(), &course_id)
                .await
                .map_err(|e| ApiError::internal(e, "Failed to load published snapshot"))?
                .filter(|p| p.enabled)
                .ok_or_else(|| ApiError::NotFound("Ассистент не опубликован".into()))?
                .snapshot
        }
    };
    let ai = AiGradingService::for_snapshot(state.settings(), &snapshot)
        .map_err(|e| ApiError::UnprocessableEntity(e.to_string()))?;
    let (rubric, max_score) = crate::services::ai_grading::bank_rubric(&snapshot)
        .map_err(|e| ApiError::UnprocessableEntity(e.to_string()))?;
    let _permit = state.assistant_chat_capacity().try_acquire().map_err(|_| {
        ApiError::ServiceUnavailable("Проверка сейчас занята. Повторите позже".into())
    })?;
    let result = ai
        .run_precheck(LlmPrecheckRequest {
            submission_id: None,
            snapshot: Some(snapshot.clone()),
            ocr_markdown_pages: vec![body.student_text],
            ocr_report_issues: vec![],
            report_summary: None,
            task_description: task.text.clone(),
            reference_solution: reference.into(),
            rubric: json!({"criteria":rubric,"total_max_score":max_score}),
            max_score,
            chemistry_rules: Some(json!({"task_count":1,"numeric_answers":[]})),
        })
        .await
        .map_err(|e| {
            tracing::error!(error=%e,"Studio grading preview failed");
            ApiError::ServiceUnavailable(
                "Проверка не завершена или ответ модели не прошёл контракт. Повторите попытку."
                    .into(),
            )
        })?;
    Ok(Json(
        json!({"output":result,"task_id":task.id,"task_number":task.number,"task_text":task.text,"reference_solution":reference,"rubric":rubric,"max_score":max_score,"snapshot_version":snapshot["version"],"draft":draft}),
    ))
}

async fn bank_image(
    Path((course_id, item_id, image_id)): Path<(String, String, String)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    authenticate_studio(&state, &headers)?;
    if !repositories::task_bank::course_has_item(state.db(), &course_id, &item_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to validate bank course"))?
    {
        return Err(ApiError::NotFound("Изображение не найдено".into()));
    }
    let image = repositories::task_bank::find_item_image(state.db(), &item_id, &image_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to read bank image"))?
        .ok_or_else(|| ApiError::NotFound("Изображение не найдено".into()))?;
    let path = crate::services::materials::resolve_task_bank_media_path(
        state.settings(),
        &image.relative_path,
    )
    .map_err(|_| ApiError::NotFound("Изображение недоступно".into()))?;
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to read bank image file"))?;
    let mime = crate::services::materials::detect_image_mime(&bytes)
        .map_err(|_| ApiError::NotFound("Изображение недоступно".into()))?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, mime),
            (axum::http::header::CACHE_CONTROL, "private, no-store"),
        ],
        bytes,
    ))
}
