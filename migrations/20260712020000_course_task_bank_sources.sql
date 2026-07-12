CREATE TABLE IF NOT EXISTS course_task_bank_sources (
    course_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT (NOW() AT TIME ZONE 'utc'),
    CONSTRAINT pk_course_task_bank_sources PRIMARY KEY (course_id, source_id),
    CONSTRAINT fk_course_task_bank_sources_course
        FOREIGN KEY (course_id) REFERENCES courses (id) ON DELETE CASCADE,
    CONSTRAINT fk_course_task_bank_sources_source
        FOREIGN KEY (source_id) REFERENCES task_bank_sources (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS ix_course_task_bank_sources_source_id
    ON course_task_bank_sources (source_id);

-- The bundled Sviridov bank is a general/inorganic chemistry source. Existing
-- analytical and future colloid courses must not inherit it merely because it
-- is active globally.
INSERT INTO course_task_bank_sources (course_id, source_id)
SELECT c.id, s.id
FROM courses c
JOIN task_bank_sources s ON s.code = 'sviridov'
WHERE LOWER(c.title) ~ '(неорган|общая.{0,20}хим|infochem)'
ON CONFLICT DO NOTHING;
