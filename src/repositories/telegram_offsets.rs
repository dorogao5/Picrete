use sqlx::PgPool;

pub(crate) async fn get_update_offset(
    pool: &PgPool,
    bot_name: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT update_offset
         FROM telegram_bot_offsets
         WHERE bot_name = $1",
    )
    .bind(bot_name)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn upsert_update_offset(
    pool: &PgPool,
    bot_name: &str,
    update_offset: i64,
    updated_at: time::PrimitiveDateTime,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO telegram_bot_offsets (bot_name, update_offset, updated_at)
         VALUES ($1, $2, $3)
         ON CONFLICT (bot_name) DO UPDATE
         SET update_offset = GREATEST(telegram_bot_offsets.update_offset, EXCLUDED.update_offset),
             updated_at = EXCLUDED.updated_at",
    )
    .bind(bot_name)
    .bind(update_offset)
    .bind(updated_at)
    .execute(pool)
    .await?;

    Ok(())
}
