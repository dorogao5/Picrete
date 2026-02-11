CREATE TABLE IF NOT EXISTS telegram_user_links (
    telegram_user_id     BIGINT      NOT NULL,
    user_id              TEXT        NOT NULL,
    telegram_username    TEXT,
    telegram_first_name  TEXT,
    linked_at            TIMESTAMP   NOT NULL DEFAULT now(),
    last_seen_at         TIMESTAMP   NOT NULL DEFAULT now(),

    CONSTRAINT pk_telegram_user_links PRIMARY KEY (telegram_user_id),
    CONSTRAINT uq_telegram_user_links_user UNIQUE (user_id),
    CONSTRAINT fk_telegram_user_links_user FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_telegram_user_links_user_id
    ON telegram_user_links (user_id);
