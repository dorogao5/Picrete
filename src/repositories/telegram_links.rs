use sqlx::PgPool;

use crate::db::models::TelegramUserLink;

const COLUMNS: &str = "\
    telegram_user_id, user_id, telegram_username, telegram_first_name, linked_at, last_seen_at";

pub(crate) async fn find_by_telegram_user_id(
    pool: &PgPool,
    telegram_user_id: i64,
) -> Result<Option<TelegramUserLink>, sqlx::Error> {
    sqlx::query_as::<_, TelegramUserLink>(&format!(
        "SELECT {COLUMNS}
         FROM telegram_user_links
         WHERE telegram_user_id = $1"
    ))
    .bind(telegram_user_id)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn upsert_link(
    pool: &PgPool,
    telegram_user_id: i64,
    user_id: &str,
    telegram_username: Option<&str>,
    telegram_first_name: Option<&str>,
    now: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO telegram_user_links (
            telegram_user_id, user_id, telegram_username, telegram_first_name, linked_at, last_seen_at
         ) VALUES ($1,$2,$3,$4,$5,$6)
         ON CONFLICT (telegram_user_id)
         DO UPDATE SET
            user_id = EXCLUDED.user_id,
            telegram_username = EXCLUDED.telegram_username,
            telegram_first_name = EXCLUDED.telegram_first_name,
            last_seen_at = EXCLUDED.last_seen_at",
    )
    .bind(telegram_user_id)
    .bind(user_id)
    .bind(telegram_username)
    .bind(telegram_first_name)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn touch_last_seen(
    pool: &PgPool,
    telegram_user_id: i64,
    now: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE telegram_user_links
         SET last_seen_at = $1
         WHERE telegram_user_id = $2",
    )
    .bind(now)
    .bind(telegram_user_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn delete_by_telegram_user_id(
    pool: &PgPool,
    telegram_user_id: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM telegram_user_links WHERE telegram_user_id = $1")
        .bind(telegram_user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
