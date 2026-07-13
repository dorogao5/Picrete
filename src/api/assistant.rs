use axum::{
    extract::{Path, State},
    http::{header, HeaderMap},
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::api::errors::ApiError;
use crate::api::guards::{require_course_membership, CurrentUser};
use crate::core::state::AppState;
use crate::core::time::{format_primitive, primitive_now_utc};
use crate::repositories;
use crate::services::assistant_chat::{AssistantChatService, PublishedRuntimePolicy};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(status))
        .route("/threads", get(list_threads))
        .route("/threads/:thread_id", get(get_thread))
        .route("/chat", axum::routing::post(chat))
}

pub(crate) fn internal_router() -> Router<AppState> {
    Router::new()
        .route("/course-assistants/:course_id", put(publish_snapshot))
        .route("/course-options", get(course_options))
}

#[derive(Debug, Deserialize, Serialize)]
struct PublishSnapshot {
    schema_version: u32,
    version: String,
    assistant: PublishedAssistant,
    prompts: Value,
    reference_sheets: Vec<Value>,
    published_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct PublishedAssistant {
    id: String,
    name: String,
    discipline: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    audience: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    criteria: Vec<Value>,
    #[serde(default)]
    nuances: Vec<String>,
    #[serde(default)]
    runtime_policy: PublishedRuntimePolicy,
}

#[derive(Debug, Serialize)]
struct AssistantStatus {
    available: bool,
    name: Option<String>,
    discipline: Option<String>,
    snapshot_version: Option<String>,
    synced_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    thread_id: Option<String>,
    message: String,
}

#[derive(Debug, Serialize)]
struct CourseOption {
    id: String,
    title: String,
    organization: Option<String>,
}

#[derive(Debug, Serialize)]
struct ThreadResponse {
    id: String,
    title: String,
    messages: Value,
    snapshot_version: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct ThreadSummaryResponse {
    id: String,
    title: String,
    snapshot_version: String,
    created_at: String,
    updated_at: String,
}

async fn publish_snapshot(
    Path(course_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PublishSnapshot>,
) -> Result<Json<Value>, ApiError> {
    authenticate_studio(&state, &headers)?;
    if payload.schema_version != 1
        || payload.version.len() != 64
        || payload.assistant.id.len() > 64
        || payload.assistant.name.trim().is_empty()
        || payload.assistant.name.len() > 256
        || payload.assistant.discipline.len() > 256
        || payload.reference_sheets.len() > 200
        || payload.published_at.len() > 64
        || payload.prompts.pointer("/tutor/system_prompt").and_then(Value::as_str).is_none()
    {
        return Err(ApiError::UnprocessableEntity("Некорректный снимок ассистента".to_string()));
    }
    if let Err(error) = payload
        .assistant
        .runtime_policy
        .validate_configured_model(&state.settings().ai().assistant_model)
    {
        return Err(ApiError::UnprocessableEntity(format!(
            "Политика модели ассистента несовместима с Picrete: {error}"
        )));
    }
    if payload.assistant.runtime_policy.is_legacy() {
        tracing::warn!(course_id = %course_id, "Publishing legacy assistant snapshot without runtime policy");
    }
    repositories::courses::find_by_id(state.db(), &course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to validate assistant course"))?
        .ok_or_else(|| ApiError::NotFound("Курс Picrete не найден".to_string()))?;
    let snapshot = serde_json::to_value(&payload)
        .map_err(|e| ApiError::internal(e, "Failed to serialize assistant snapshot"))?;
    if snapshot.to_string().len() > 1_600_000 {
        return Err(ApiError::UnprocessableEntity("Снимок ассистента слишком большой".to_string()));
    }
    let synced_at = primitive_now_utc();
    let stored = repositories::course_ai_assistants::upsert(
        state.db(),
        &course_id,
        &payload.assistant.id,
        payload.assistant.name.trim(),
        payload.assistant.discipline.trim(),
        &payload.version,
        snapshot,
        synced_at,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to publish course assistant"))?;
    tracing::info!(course_id = %course_id, snapshot_version = %stored.snapshot_version, "Studio assistant published");
    Ok(Json(json!({"ok": true, "synced_at": format_primitive(stored.synced_at)})))
}

fn authenticate_studio(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let expected = state.settings().studio_integration().token.as_bytes();
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("")
        .as_bytes();
    if expected.is_empty() || Sha256::digest(expected) != Sha256::digest(provided) {
        return Err(ApiError::Unauthorized("Invalid Studio integration credentials"));
    }
    Ok(())
}

async fn course_options(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<CourseOption>>, ApiError> {
    authenticate_studio(&state, &headers)?;
    let rows = sqlx::query_as::<_, (String, String, Option<String>)>(
        "SELECT id, title, organization FROM courses WHERE is_active = TRUE ORDER BY title",
    )
    .fetch_all(state.db())
    .await
    .map_err(|e| ApiError::internal(e, "Failed to list Picrete courses"))?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, title, organization)| CourseOption { id, title, organization })
            .collect(),
    ))
}

async fn status(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<AssistantStatus>, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;
    let assistant = repositories::course_ai_assistants::find(state.db(), &course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load course assistant"))?;
    Ok(Json(match assistant.filter(|item| item.enabled) {
        Some(item) => AssistantStatus {
            available: true,
            name: Some(item.name),
            discipline: Some(item.discipline),
            snapshot_version: Some(item.snapshot_version),
            synced_at: Some(format_primitive(item.synced_at)),
        },
        None => AssistantStatus {
            available: false,
            name: None,
            discipline: None,
            snapshot_version: None,
            synced_at: None,
        },
    }))
}

async fn list_threads(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ThreadSummaryResponse>>, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;
    let threads =
        repositories::course_ai_assistants::list_threads(state.db(), &course_id, &user.id)
            .await
            .map_err(|e| ApiError::internal(e, "Failed to list assistant chats"))?;
    Ok(Json(threads.into_iter().map(thread_summary_response).collect()))
}

async fn get_thread(
    Path((course_id, thread_id)): Path<(String, String)>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> Result<Json<ThreadResponse>, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;
    let thread = repositories::course_ai_assistants::find_thread(
        state.db(),
        &thread_id,
        &course_id,
        &user.id,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to load assistant chat"))?
    .ok_or_else(|| ApiError::NotFound("Диалог не найден".to_string()))?;
    Ok(Json(thread_response(thread)))
}

async fn chat(
    Path(course_id): Path<String>,
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(payload): Json<ChatRequest>,
) -> Result<Json<ThreadResponse>, ApiError> {
    require_course_membership(&state, &user, &course_id).await?;
    let message = payload.message.trim();
    if message.is_empty() || message.chars().count() > 4_000 {
        return Err(ApiError::BadRequest(
            "Сообщение должно содержать от 1 до 4000 символов".to_string(),
        ));
    }
    let rate_key = format!("assistant-chat:{}:{}", course_id, user.id);
    match state.redis().rate_limit(&rate_key, 12, 60).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(ApiError::TooManyRequests("Слишком много сообщений. Подождите минуту."))
        }
        Err(error) => {
            tracing::error!(%error, "Assistant chat rate limit failed");
            return Err(ApiError::ServiceUnavailable("Диалог временно недоступен".to_string()));
        }
    }
    let assistant = repositories::course_ai_assistants::find(state.db(), &course_id)
        .await
        .map_err(|e| ApiError::internal(e, "Failed to load course assistant"))?
        .filter(|item| item.enabled)
        .ok_or_else(|| {
            ApiError::NotFound("Для курса ещё не опубликован ИИ-ассистент".to_string())
        })?;

    let thread_id = payload.thread_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let existing = repositories::course_ai_assistants::find_thread(
        state.db(),
        &thread_id,
        &course_id,
        &user.id,
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to load assistant chat"))?;
    ensure_thread_snapshot_is_current(
        existing.as_ref().map(|thread| thread.snapshot_version.as_str()),
        &assistant.snapshot_version,
    )?;
    let mut messages = existing
        .as_ref()
        .and_then(|thread| thread.messages.as_array().cloned())
        .unwrap_or_default();
    messages.push(json!({"role": "user", "content": message}));
    let service = AssistantChatService::from_settings(state.settings())
        .map_err(|e| ApiError::internal(e, "Failed to initialize course assistant"))?;
    let reply = service.reply(&assistant.snapshot, &messages).await.map_err(|error| {
        tracing::error!(%error, course_id = %course_id, "Course assistant reply failed");
        ApiError::ServiceUnavailable(
            "Ассистент не смог ответить. Сообщение не сохранено — повторите попытку.".to_string(),
        )
    })?;
    messages.push(json!({"role": "assistant", "content": reply}));
    if messages.len() > 60 {
        messages.drain(0..messages.len() - 60);
    }
    let title = existing.as_ref().map(|thread| thread.title.clone()).unwrap_or_else(|| {
        let mut value = message.chars().take(70).collect::<String>();
        if message.chars().count() > 70 {
            value.push('…');
        }
        value
    });
    let saved = repositories::course_ai_assistants::save_thread(
        state.db(),
        &thread_id,
        &course_id,
        &user.id,
        &title,
        Value::Array(messages),
        &assistant.snapshot_version,
        primitive_now_utc(),
    )
    .await
    .map_err(|e| ApiError::internal(e, "Failed to save assistant chat"))?;
    Ok(Json(thread_response(saved)))
}

fn ensure_thread_snapshot_is_current(
    thread_snapshot_version: Option<&str>,
    current_snapshot_version: &str,
) -> Result<(), ApiError> {
    if thread_snapshot_version.is_some_and(|version| version != current_snapshot_version) {
        return Err(ApiError::Conflict(
            "Ассистент курса обновился. Начните новый диалог, чтобы использовать актуальную версию."
                .to_string(),
        ));
    }
    Ok(())
}

fn thread_response(
    thread: repositories::course_ai_assistants::AssistantChatThread,
) -> ThreadResponse {
    ThreadResponse {
        id: thread.id,
        title: thread.title,
        messages: thread.messages,
        snapshot_version: thread.snapshot_version,
        created_at: format_primitive(thread.created_at),
        updated_at: format_primitive(thread.updated_at),
    }
}

fn thread_summary_response(
    thread: repositories::course_ai_assistants::AssistantChatThreadSummary,
) -> ThreadSummaryResponse {
    ThreadSummaryResponse {
        id: thread.id,
        title: thread.title,
        snapshot_version: thread.snapshot_version,
        created_at: format_primitive(thread.created_at),
        updated_at: format_primitive(thread.updated_at),
    }
}

#[cfg(test)]
mod tests {
    use super::{ensure_thread_snapshot_is_current, PublishSnapshot};
    use crate::api::errors::ApiError;
    use serde_json::json;

    fn base_snapshot(assistant: serde_json::Value) -> serde_json::Value {
        json!({
            "schema_version": 1,
            "version": "0".repeat(64),
            "assistant": assistant,
            "prompts": {"tutor": {"system_prompt": "Помогайте студенту"}},
            "reference_sheets": [],
            "published_at": "2026-07-13T00:00:00Z"
        })
    }

    #[test]
    fn old_snapshots_deserialize_with_empty_profile_defaults() {
        let snapshot: PublishSnapshot = serde_json::from_value(base_snapshot(json!({
            "id": "assistant-1",
            "name": "Ассистент",
            "discipline": "Химия"
        })))
        .expect("legacy snapshot must remain valid");

        assert!(snapshot.assistant.description.is_empty());
        assert!(snapshot.assistant.audience.is_empty());
        assert!(snapshot.assistant.topics.is_empty());
        assert!(snapshot.assistant.criteria.is_empty());
        assert!(snapshot.assistant.nuances.is_empty());
        assert!(snapshot.assistant.runtime_policy.is_legacy());
    }

    #[test]
    fn published_profile_fields_survive_snapshot_round_trip() {
        let mut raw = base_snapshot(json!({
            "id": "assistant-1",
            "name": "Практикум",
            "discipline": "Неорганическая химия",
            "description": "Первый курс",
            "audience": "студенты 1 курса",
            "language": "ru",
            "topics": ["Растворы"],
            "criteria": [{"name": "Расчёт", "max_score": 4}],
            "nuances": ["Не придумывать наблюдения"]
        }));
        raw["assistant"]["runtime_policy"] = json!({
            "policy_version": "model-use-v1:test",
            "tutor_model_id": "deepseek-v4-pro",
            "decision_model_id": "deepseek-v4-pro",
            "tier": "decision",
            "allowed_uses": ["student_tutor", "task_validation", "grading"]
        });
        let snapshot: PublishSnapshot =
            serde_json::from_value(raw).expect("extended snapshot must be valid");
        let encoded = serde_json::to_value(snapshot).expect("snapshot must serialize");

        assert_eq!(encoded["assistant"]["audience"], "студенты 1 курса");
        assert_eq!(encoded["assistant"]["topics"], json!(["Растворы"]));
        assert_eq!(encoded["assistant"]["criteria"][0]["max_score"], 4);
        assert_eq!(encoded["assistant"]["nuances"][0], "Не придумывать наблюдения");
        assert_eq!(encoded["assistant"]["runtime_policy"]["tutor_model_id"], "deepseek-v4-pro");
        assert_eq!(encoded["assistant"]["runtime_policy"]["tier"], "decision");
    }

    #[test]
    fn stale_thread_snapshot_is_rejected_before_continuation() {
        let error = ensure_thread_snapshot_is_current(Some("snapshot-v1"), "snapshot-v2")
            .expect_err("stale thread must not continue with a newer assistant snapshot");

        match error {
            ApiError::Conflict(message) => {
                assert!(message.contains("Начните новый диалог"));
            }
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn current_or_new_thread_snapshot_is_allowed() {
        ensure_thread_snapshot_is_current(Some("snapshot-v2"), "snapshot-v2")
            .expect("current thread must remain writable");
        ensure_thread_snapshot_is_current(None, "snapshot-v2")
            .expect("new thread must remain writable");
    }
}
