CREATE TABLE IF NOT EXISTS telegram_selected_sessions (
    telegram_user_id  BIGINT      NOT NULL,
    course_id         TEXT        NOT NULL,
    session_id        TEXT        NOT NULL,
    selected_at       TIMESTAMP   NOT NULL DEFAULT now(),

    CONSTRAINT pk_telegram_selected_sessions PRIMARY KEY (telegram_user_id),
    CONSTRAINT fk_telegram_selected_sessions_link
        FOREIGN KEY (telegram_user_id) REFERENCES telegram_user_links (telegram_user_id) ON DELETE CASCADE,
    CONSTRAINT fk_telegram_selected_sessions_session
        FOREIGN KEY (session_id, course_id) REFERENCES exam_sessions (id, course_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_telegram_selected_sessions_course_session
    ON telegram_selected_sessions (course_id, session_id);
