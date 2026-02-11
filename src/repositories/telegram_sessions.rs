use sqlx::PgPool;

use crate::db::models::TelegramSelectedSession;

const COLUMNS: &str = "telegram_user_id, course_id, session_id, selected_at";

pub(crate) async fn get_selected(
    pool: &PgPool,
    telegram_user_id: i64,
) -> Result<Option<TelegramSelectedSession>, sqlx::Error> {
    sqlx::query_as::<_, TelegramSelectedSession>(&format!(
        "SELECT {COLUMNS}
         FROM telegram_selected_sessions
         WHERE telegram_user_id = $1"
    ))
    .bind(telegram_user_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn upsert_selected(
    pool: &PgPool,
    telegram_user_id: i64,
    course_id: &str,
    session_id: &str,
    now: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO telegram_selected_sessions (
            telegram_user_id, course_id, session_id, selected_at
         ) VALUES ($1,$2,$3,$4)
         ON CONFLICT (telegram_user_id)
         DO UPDATE SET
            course_id = EXCLUDED.course_id,
            session_id = EXCLUDED.session_id,
            selected_at = EXCLUDED.selected_at",
    )
    .bind(telegram_user_id)
    .bind(course_id)
    .bind(session_id)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn clear_selected(
    pool: &PgPool,
    telegram_user_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM telegram_selected_sessions WHERE telegram_user_id = $1")
        .bind(telegram_user_id)
        .execute(pool)
        .await?;
    Ok(())
}
