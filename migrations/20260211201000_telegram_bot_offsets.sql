CREATE TABLE IF NOT EXISTS telegram_bot_offsets (
    bot_name TEXT PRIMARY KEY,
    update_offset BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

