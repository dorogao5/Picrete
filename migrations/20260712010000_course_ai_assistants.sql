CREATE TABLE course_ai_assistants (
    course_id            TEXT      NOT NULL,
    studio_assistant_id  TEXT      NOT NULL,
    name                 TEXT      NOT NULL,
    discipline           TEXT      NOT NULL,
    snapshot_version     TEXT      NOT NULL,
    snapshot             JSONB     NOT NULL,
    enabled              BOOLEAN   NOT NULL DEFAULT TRUE,
    synced_at            TIMESTAMP NOT NULL DEFAULT now(),

    CONSTRAINT pk_course_ai_assistants PRIMARY KEY (course_id),
    CONSTRAINT fk_course_ai_assistants_course
        FOREIGN KEY (course_id) REFERENCES courses (id) ON DELETE CASCADE
);

CREATE TABLE assistant_chat_threads (
    id                TEXT      NOT NULL,
    course_id         TEXT      NOT NULL,
    user_id           TEXT      NOT NULL,
    title             TEXT      NOT NULL,
    messages          JSONB     NOT NULL DEFAULT '[]'::jsonb,
    snapshot_version  TEXT      NOT NULL,
    created_at        TIMESTAMP NOT NULL DEFAULT now(),
    updated_at        TIMESTAMP NOT NULL DEFAULT now(),

    CONSTRAINT pk_assistant_chat_threads PRIMARY KEY (id),
    CONSTRAINT fk_assistant_chat_threads_course
        FOREIGN KEY (course_id) REFERENCES courses (id) ON DELETE CASCADE,
    CONSTRAINT fk_assistant_chat_threads_user
        FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE INDEX ix_assistant_chat_threads_user_course_updated
    ON assistant_chat_threads (user_id, course_id, updated_at DESC);
