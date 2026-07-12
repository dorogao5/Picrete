use serde_json::Value;
use sqlx::{FromRow, PgPool};
use time::PrimitiveDateTime;

#[derive(Debug, Clone, FromRow)]
pub(crate) struct CourseAiAssistant {
    pub(crate) name: String,
    pub(crate) discipline: String,
    pub(crate) snapshot_version: String,
    pub(crate) snapshot: Value,
    pub(crate) enabled: bool,
    pub(crate) synced_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct AssistantChatThread {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) messages: Value,
    pub(crate) snapshot_version: String,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

#[derive(Debug, Clone, FromRow)]
pub(crate) struct AssistantChatThreadSummary {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) snapshot_version: String,
    pub(crate) created_at: PrimitiveDateTime,
    pub(crate) updated_at: PrimitiveDateTime,
}

pub(crate) async fn upsert(
    pool: &PgPool,
    course_id: &str,
    studio_assistant_id: &str,
    name: &str,
    discipline: &str,
    snapshot_version: &str,
    snapshot: Value,
    synced_at: PrimitiveDateTime,
) -> Result<CourseAiAssistant, sqlx::Error> {
    sqlx::query_as::<_, CourseAiAssistant>(
        "INSERT INTO course_ai_assistants
            (course_id, studio_assistant_id, name, discipline, snapshot_version, snapshot, enabled, synced_at)
         VALUES ($1, $2, $3, $4, $5, $6, TRUE, $7)
         ON CONFLICT (course_id) DO UPDATE SET
            studio_assistant_id = EXCLUDED.studio_assistant_id,
            name = EXCLUDED.name,
            discipline = EXCLUDED.discipline,
            snapshot_version = EXCLUDED.snapshot_version,
            snapshot = EXCLUDED.snapshot,
            enabled = TRUE,
            synced_at = EXCLUDED.synced_at
         RETURNING name, discipline, snapshot_version,
                   snapshot, enabled, synced_at",
    )
    .bind(course_id)
    .bind(studio_assistant_id)
    .bind(name)
    .bind(discipline)
    .bind(snapshot_version)
    .bind(snapshot)
    .bind(synced_at)
    .fetch_one(pool)
    .await
}

pub(crate) async fn find(
    pool: &PgPool,
    course_id: &str,
) -> Result<Option<CourseAiAssistant>, sqlx::Error> {
    sqlx::query_as::<_, CourseAiAssistant>(
        "SELECT name, discipline, snapshot_version,
                snapshot, enabled, synced_at
         FROM course_ai_assistants WHERE course_id = $1",
    )
    .bind(course_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn list_threads(
    pool: &PgPool,
    course_id: &str,
    user_id: &str,
) -> Result<Vec<AssistantChatThreadSummary>, sqlx::Error> {
    sqlx::query_as::<_, AssistantChatThreadSummary>(
        "SELECT id, title, snapshot_version, created_at, updated_at
         FROM assistant_chat_threads
         WHERE course_id = $1 AND user_id = $2
         ORDER BY updated_at DESC LIMIT 30",
    )
    .bind(course_id)
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub(crate) async fn find_thread(
    pool: &PgPool,
    thread_id: &str,
    course_id: &str,
    user_id: &str,
) -> Result<Option<AssistantChatThread>, sqlx::Error> {
    sqlx::query_as::<_, AssistantChatThread>(
        "SELECT id, title, messages, snapshot_version, created_at, updated_at
         FROM assistant_chat_threads
         WHERE id = $1 AND course_id = $2 AND user_id = $3",
    )
    .bind(thread_id)
    .bind(course_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn save_thread(
    pool: &PgPool,
    thread_id: &str,
    course_id: &str,
    user_id: &str,
    title: &str,
    messages: Value,
    snapshot_version: &str,
    now: PrimitiveDateTime,
) -> Result<AssistantChatThread, sqlx::Error> {
    sqlx::query_as::<_, AssistantChatThread>(
        "INSERT INTO assistant_chat_threads
            (id, course_id, user_id, title, messages, snapshot_version, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
         ON CONFLICT (id) DO UPDATE SET
            title = EXCLUDED.title,
            messages = EXCLUDED.messages,
            snapshot_version = EXCLUDED.snapshot_version,
            updated_at = EXCLUDED.updated_at
         WHERE assistant_chat_threads.course_id = EXCLUDED.course_id
           AND assistant_chat_threads.user_id = EXCLUDED.user_id
         RETURNING id, title, messages, snapshot_version, created_at, updated_at",
    )
    .bind(thread_id)
    .bind(course_id)
    .bind(user_id)
    .bind(title)
    .bind(messages)
    .bind(snapshot_version)
    .bind(now)
    .fetch_one(pool)
    .await
}
