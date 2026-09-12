//! Teacher-only Studio bridge. Uses the production grading engine without submissions.
use crate::{
    api::{assistant::authenticate_studio, errors::ApiError},
    core::state::AppState,
    core::time::primitive_now_utc,
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
        .route("/courses/:course_id/task-bank/import", post(import_bank))
        .route("/courses/:course_id/grading-preview", post(preview))
        .route("/courses/:course_id/task-bank/:item_id/images/:image_id", get(bank_image))
}

#[derive(Debug, Deserialize)]
struct TaskBankExport {
    source: TaskBankExportSource,
    #[serde(default)]
    paragraphs: Vec<TaskBankExportParagraph>,
}

#[derive(Debug, Deserialize)]
struct TaskBankExportSource {
    code: String,
    title: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct TaskBankExportParagraph {
    paragraph: String,
    topic: String,
    #[serde(default)]
    theory_text: String,
    #[serde(default)]
    tasks: Vec<TaskBankExportTask>,
}

#[derive(Debug, Deserialize)]
struct TaskBankExportTask {
    number: String,
    text: String,
    #[serde(default)]
    solution: String,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    difficulty: Option<String>,
    #[serde(default)]
    volume: Option<String>,
    #[serde(default)]
    task_type: Option<String>,
    #[serde(default)]
    images: Vec<String>,
}

async fn import_bank(
    Path(course_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<TaskBankExport>,
) -> Result<Json<Value>, ApiError> {
    authenticate_studio(&state, &headers)?;
    let source_code = payload.source.code.trim().to_string();
    let source_title = payload.source.title.trim().to_string();
    let source_version = payload.source.version.trim().to_string();
    if source_code.is_empty()
        || source_code.len() > 128
        || source_title.is_empty()
        || source_title.len() > 256
        || source_version.is_empty()
        || source_version.len() > 64
    {
        return Err(ApiError::UnprocessableEntity(
            "Некорректные метаданные источника банка задач".into(),
        ));
    }
    if payload.paragraphs.is_empty() || payload.paragraphs.len() > 1000 {
        return Err(ApiError::UnprocessableEntity(
            "Экспорт должен содержать от 1 до 1000 разделов".into(),
        ));
    }
    repositories::courses::find_by_id(state.db(), &course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to validate task bank course"))?
        .ok_or_else(|| ApiError::NotFound("Курс Picrete не найден".into()))?;

    let source_id = format!("tb_source_{}", sanitize_id_fragment(&source_code));
    let now = primitive_now_utc();
    let mut tx = state
        .db()
        .begin()
        .await
        .map_err(|e| ApiError::internal(e, "Failed to begin task bank import"))?;
    let source = repositories::task_bank::upsert_source(
        &mut *tx,
        repositories::task_bank::UpsertSource {
            id: &source_id,
            code: &source_code,
            title: &source_title,
            version: &source_version,
            is_active: true,
            now,
        },
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to upsert imported task bank source"))?;
    sqlx::query(
        "INSERT INTO course_task_bank_sources (course_id, source_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(&course_id)
    .bind(&source.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError::internal(e, "Failed to scope imported task bank source"))?;

    let mut imported_items = 0usize;
    let imported_images = 0usize;
    for paragraph in payload.paragraphs {
        let paragraph_value = paragraph.paragraph.trim();
        let topic_value = paragraph.topic.trim();
        if paragraph_value.is_empty() || topic_value.is_empty() {
            return Err(ApiError::UnprocessableEntity(
                "У импортируемой задачи отсутствует раздел или тема".into(),
            ));
        }
        for task in paragraph.tasks {
            let number = task.number.trim();
            let text = task.text.trim();
            let solution = task.solution.trim();
            let answer = task.answer.trim();
            if number.is_empty() || text.is_empty() || solution.is_empty() || answer.is_empty() {
                return Err(ApiError::UnprocessableEntity(format!(
                    "Неполная задача {number}: нужны условие, эталонное решение и ответ"
                )));
            }
            if !task.images.is_empty() {
                return Err(ApiError::UnprocessableEntity(format!("Задача {number} содержит изображения; импорт изображений должен быть выполнен отдельным защищённым пакетом")));
            }
            let item_id = format!(
                "tb_{}_{}",
                sanitize_id_fragment(&source_code),
                sanitize_id_fragment(number)
            );
            repositories::task_bank::upsert_item(
                &mut *tx,
                repositories::task_bank::UpsertItem {
                    id: &item_id,
                    source_id: &source.id,
                    number,
                    paragraph: paragraph_value,
                    topic: topic_value,
                    solution: Some(solution),
                    task_type: task.task_type.as_deref().and_then(valid_task_type),
                    difficulty: task.difficulty.as_deref().and_then(valid_difficulty),
                    volume: task.volume.as_deref().and_then(valid_volume),
                    text,
                    answer: Some(answer),
                    has_answer: true,
                    metadata: json!({
                        "origin": "picrete_studio",
                        "source_version": &source_version,
                        "theory_text": paragraph.theory_text,
                    }),
                    now,
                },
            )
            .await
            .map_err(|e| ApiError::internal(e, "Failed to import task bank item"))?;
            repositories::task_bank::replace_item_images(&mut tx, &item_id, &[])
                .await
                .map_err(|e| ApiError::internal(e, "Failed to finalize task bank item"))?;
            imported_items += 1;
        }
    }
    tx.commit().await.map_err(|e| ApiError::internal(e, "Failed to commit task bank import"))?;
    tracing::info!(course_id = %course_id, source_code = %source_code, imported_items, imported_images, "Studio task bank imported");
    Ok(Json(
        json!({"ok": true, "source_code": source_code, "imported_items": imported_items, "imported_images": imported_images}),
    ))
}

fn valid_difficulty(value: &str) -> Option<&str> {
    match value.trim() {
        "easy" | "medium" | "hard" => Some(value.trim()),
        _ => None,
    }
}

fn valid_volume(value: &str) -> Option<&str> {
    match value.trim() {
        "small" | "medium" | "large" => Some(value.trim()),
        _ => None,
    }
}

fn valid_task_type(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 64).then_some(value)
}

fn sanitize_id_fragment(raw: &str) -> String {
    raw.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect()
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
            // The Studio course-bank picker is also used for subject-specific
            // imported banks. Restricting it to the legacy source made the
            // picker empty for a new course and blocked grading preflight.
            source_code: None,
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
