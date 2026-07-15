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
    find_with_executor(pool, course_id).await
}

pub(crate) async fn find_with_executor(
    executor: impl sqlx::PgExecutor<'_>,
    course_id: &str,
) -> Result<Option<CourseAiAssistant>, sqlx::Error> {
    sqlx::query_as::<_, CourseAiAssistant>(
        "SELECT name, discipline, snapshot_version,
                snapshot, enabled, synced_at
         FROM course_ai_assistants WHERE course_id = $1",
    )
    .bind(course_id)
    .fetch_optional(executor)
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
    find_thread_with_executor(pool, thread_id, course_id, user_id).await
}

pub(crate) async fn find_thread_with_executor(
    executor: impl sqlx::PgExecutor<'_>,
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
    .fetch_optional(executor)
    .await
}

pub(crate) async fn acquire_thread_lock(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    course_id: &str,
    user_id: &str,
    thread_id: &str,
    wait_timeout_seconds: u64,
) -> Result<(), sqlx::Error> {
    let lock_timeout = format!("{}s", wait_timeout_seconds.max(1));
    sqlx::query("SELECT set_config('lock_timeout', $1, true)")
        .bind(lock_timeout)
        .execute(&mut **transaction)
        .await?;

    // The namespace prefix isolates chat locks from the advisory locks used by
    // exam-session creation. A transaction-scoped lock is released on every
    // success/error/cancellation path, including an upstream model timeout.
    sqlx::query(
        "SELECT pg_advisory_xact_lock(
            hashtextextended('assistant-chat:' || $1 || ':' || $2 || ':' || $3, 0)
         )",
    )
    .bind(course_id)
    .bind(user_id)
    .bind(thread_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_thread_with_executor(
    executor: impl sqlx::PgExecutor<'_>,
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
    .fetch_one(executor)
    .await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{json, Value};
    use tokio::sync::oneshot;
    use tokio::time::timeout;
    use uuid::Uuid;

    use super::{
        acquire_thread_lock, find_thread, find_thread_with_executor, save_thread_with_executor,
    };
    use crate::core::time::primitive_now_utc;
    use crate::test_support;

    #[tokio::test]
    async fn concurrent_thread_writers_are_serialized_without_lost_messages() {
        let context = test_support::setup_test_context().await;
        let user = test_support::insert_user(
            context.state.db(),
            "assistant-lock-user",
            "Assistant Lock User",
            "test-password",
        )
        .await;
        let course = test_support::insert_course(
            context.state.db(),
            "assistant-lock-course",
            "Assistant Lock Course",
            &user.id,
        )
        .await;
        let thread_id = Uuid::new_v4().to_string();
        let snapshot_version = "a".repeat(64);

        let first_pool = context.state.db().clone();
        let first_course_id = course.id.clone();
        let first_user_id = user.id.clone();
        let first_thread_id = thread_id.clone();
        let first_version = snapshot_version.clone();
        let (first_locked_tx, first_locked_rx) = oneshot::channel();
        let (release_first_tx, release_first_rx) = oneshot::channel();
        let first = tokio::spawn(async move {
            let mut transaction = first_pool.begin().await.expect("first transaction");
            acquire_thread_lock(
                &mut transaction,
                &first_course_id,
                &first_user_id,
                &first_thread_id,
                5,
            )
            .await
            .expect("first thread lock");
            save_thread_with_executor(
                &mut *transaction,
                &first_thread_id,
                &first_course_id,
                &first_user_id,
                "Диалог",
                json!([
                    {"role": "user", "content": "Первое сообщение"},
                    {"role": "assistant", "content": "Первый ответ"}
                ]),
                &first_version,
                primitive_now_utc(),
            )
            .await
            .expect("save first turn");
            first_locked_tx.send(()).expect("signal first lock");
            release_first_rx.await.expect("release first transaction");
            transaction.commit().await.expect("commit first transaction");
        });
        first_locked_rx.await.expect("first writer locked");

        let second_pool = context.state.db().clone();
        let second_course_id = course.id.clone();
        let second_user_id = user.id.clone();
        let second_thread_id = thread_id.clone();
        let second_version = snapshot_version.clone();
        let (second_attempting_tx, second_attempting_rx) = oneshot::channel();
        let (second_acquired_tx, mut second_acquired_rx) = oneshot::channel();
        let second = tokio::spawn(async move {
            let mut transaction = second_pool.begin().await.expect("second transaction");
            second_attempting_tx.send(()).expect("signal second attempt");
            acquire_thread_lock(
                &mut transaction,
                &second_course_id,
                &second_user_id,
                &second_thread_id,
                5,
            )
            .await
            .expect("second thread lock");
            second_acquired_tx.send(()).expect("signal second lock");
            let existing = find_thread_with_executor(
                &mut *transaction,
                &second_thread_id,
                &second_course_id,
                &second_user_id,
            )
            .await
            .expect("load first turn")
            .expect("thread exists");
            let mut messages = existing.messages.as_array().cloned().expect("message array");
            messages.extend([
                json!({"role": "user", "content": "Второе сообщение"}),
                json!({"role": "assistant", "content": "Второй ответ"}),
            ]);
            save_thread_with_executor(
                &mut *transaction,
                &second_thread_id,
                &second_course_id,
                &second_user_id,
                &existing.title,
                Value::Array(messages),
                &second_version,
                primitive_now_utc(),
            )
            .await
            .expect("save second turn");
            transaction.commit().await.expect("commit second transaction");
        });

        second_attempting_rx.await.expect("second writer started");
        assert!(
            timeout(Duration::from_millis(75), &mut second_acquired_rx).await.is_err(),
            "second writer must wait while the first transaction owns the thread lock"
        );
        release_first_tx.send(()).expect("release first writer");
        timeout(Duration::from_secs(2), &mut second_acquired_rx)
            .await
            .expect("second writer lock wait timed out")
            .expect("second writer lock signal dropped");
        first.await.expect("first writer task");
        second.await.expect("second writer task");

        let saved = find_thread(context.state.db(), &thread_id, &course.id, &user.id)
            .await
            .expect("load final thread")
            .expect("final thread exists");
        assert_eq!(
            saved.messages,
            json!([
                {"role": "user", "content": "Первое сообщение"},
                {"role": "assistant", "content": "Первый ответ"},
                {"role": "user", "content": "Второе сообщение"},
                {"role": "assistant", "content": "Второй ответ"}
            ])
        );
    }
}
