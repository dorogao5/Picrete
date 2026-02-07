-- ============================================================
-- Picrete — initial database schema
-- ============================================================
-- Run against a fresh PostgreSQL 15+ database.
-- All identifiers are lower-case to match the sqlx type_name
-- attributes used in the Rust backend (see db/types.rs).
-- ============================================================

-- ------------------------------------------------------------
-- 1. Custom ENUM types
-- ------------------------------------------------------------

CREATE TYPE userrole AS ENUM (
    'admin',
    'teacher',
    'assistant',
    'student'
);

CREATE TYPE examstatus AS ENUM (
    'draft',
    'published',
    'active',
    'completed',
    'archived'
);

CREATE TYPE difficultylevel AS ENUM (
    'easy',
    'medium',
    'hard'
);

CREATE TYPE sessionstatus AS ENUM (
    'active',
    'submitted',
    'expired',
    'graded'
);

CREATE TYPE submissionstatus AS ENUM (
    'uploaded',
    'processing',
    'preliminary',
    'approved',
    'flagged',
    'rejected'
);

-- ------------------------------------------------------------
-- 2. Tables
-- ------------------------------------------------------------

-- 2.1 users ---------------------------------------------------

CREATE TABLE users (
    id                  TEXT        NOT NULL,
    isu                 TEXT        NOT NULL,
    hashed_password     TEXT        NOT NULL,
    full_name           TEXT        NOT NULL,
    role                userrole    NOT NULL,
    is_active           BOOLEAN     NOT NULL DEFAULT TRUE,
    is_verified         BOOLEAN     NOT NULL DEFAULT FALSE,
    pd_consent          BOOLEAN     NOT NULL DEFAULT FALSE,
    pd_consent_at       TIMESTAMPTZ,
    pd_consent_version  TEXT,
    terms_accepted_at   TIMESTAMPTZ,
    terms_version       TEXT,
    privacy_version     TEXT,
    created_at          TIMESTAMP   NOT NULL DEFAULT now(),
    updated_at          TIMESTAMP   NOT NULL DEFAULT now(),

    CONSTRAINT pk_users PRIMARY KEY (id),
    CONSTRAINT uq_users_isu UNIQUE (isu)
);

-- 2.2 exams ---------------------------------------------------

CREATE TABLE exams (
    id                      TEXT        NOT NULL,
    title                   TEXT        NOT NULL,
    description             TEXT,
    start_time              TIMESTAMP   NOT NULL,
    end_time                TIMESTAMP   NOT NULL,
    duration_minutes        INTEGER     NOT NULL,
    timezone                TEXT        NOT NULL DEFAULT 'UTC',
    max_attempts            INTEGER     NOT NULL DEFAULT 1,
    allow_breaks            BOOLEAN     NOT NULL DEFAULT FALSE,
    break_duration_minutes  INTEGER     NOT NULL DEFAULT 0,
    auto_save_interval      INTEGER     NOT NULL DEFAULT 30,
    status                  examstatus  NOT NULL DEFAULT 'draft',
    created_by              TEXT        NOT NULL,
    created_at              TIMESTAMP   NOT NULL DEFAULT now(),
    updated_at              TIMESTAMP   NOT NULL DEFAULT now(),
    published_at            TIMESTAMP,
    settings                JSONB       NOT NULL DEFAULT '{}',

    CONSTRAINT pk_exams PRIMARY KEY (id),
    CONSTRAINT fk_exams_created_by
        FOREIGN KEY (created_by) REFERENCES users (id)
);

-- 2.3 task_types ----------------------------------------------

CREATE TABLE task_types (
    id                TEXT            NOT NULL,
    exam_id           TEXT            NOT NULL,
    title             TEXT            NOT NULL,
    description       TEXT            NOT NULL,
    order_index       INTEGER         NOT NULL DEFAULT 0,
    max_score         DOUBLE PRECISION NOT NULL DEFAULT 0,
    rubric            JSONB           NOT NULL DEFAULT '{}',
    difficulty        difficultylevel NOT NULL DEFAULT 'medium',
    taxonomy_tags     JSONB           NOT NULL DEFAULT '[]',
    formulas          JSONB           NOT NULL DEFAULT '[]',
    units             JSONB           NOT NULL DEFAULT '[]',
    validation_rules  JSONB           NOT NULL DEFAULT '{}',
    created_at        TIMESTAMP       NOT NULL DEFAULT now(),
    updated_at        TIMESTAMP       NOT NULL DEFAULT now(),

    CONSTRAINT pk_task_types PRIMARY KEY (id),
    CONSTRAINT fk_task_types_exam
        FOREIGN KEY (exam_id) REFERENCES exams (id) ON DELETE CASCADE
);

-- 2.4 task_variants -------------------------------------------

CREATE TABLE task_variants (
    id                  TEXT             NOT NULL,
    task_type_id        TEXT             NOT NULL,
    content             TEXT             NOT NULL,
    parameters          JSONB            NOT NULL DEFAULT '{}',
    reference_solution  TEXT,
    reference_answer    TEXT,
    answer_tolerance    DOUBLE PRECISION NOT NULL DEFAULT 0,
    attachments         JSONB            NOT NULL DEFAULT '[]',
    created_at          TIMESTAMP        NOT NULL DEFAULT now(),

    CONSTRAINT pk_task_variants PRIMARY KEY (id),
    CONSTRAINT fk_task_variants_task_type
        FOREIGN KEY (task_type_id) REFERENCES task_types (id) ON DELETE CASCADE
);

-- 2.5 exam_sessions -------------------------------------------

CREATE TABLE exam_sessions (
    id                    TEXT           NOT NULL,
    exam_id               TEXT           NOT NULL,
    student_id            TEXT           NOT NULL,
    variant_seed          INTEGER        NOT NULL,
    variant_assignments   JSONB          NOT NULL DEFAULT '{}',
    started_at            TIMESTAMP      NOT NULL,
    submitted_at          TIMESTAMP,
    expires_at            TIMESTAMP      NOT NULL,
    status                sessionstatus  NOT NULL DEFAULT 'active',
    attempt_number        INTEGER        NOT NULL DEFAULT 1,
    ip_address            TEXT,
    user_agent            TEXT,
    last_auto_save        TIMESTAMP,
    auto_save_data        JSONB          NOT NULL DEFAULT '{}',
    created_at            TIMESTAMP      NOT NULL DEFAULT now(),
    updated_at            TIMESTAMP      NOT NULL DEFAULT now(),

    CONSTRAINT pk_exam_sessions PRIMARY KEY (id),
    CONSTRAINT fk_exam_sessions_exam
        FOREIGN KEY (exam_id) REFERENCES exams (id) ON DELETE CASCADE,
    CONSTRAINT fk_exam_sessions_student
        FOREIGN KEY (student_id) REFERENCES users (id)
);

-- 2.6 submissions ---------------------------------------------

CREATE TABLE submissions (
    id                            TEXT              NOT NULL,
    session_id                    TEXT              NOT NULL,
    student_id                    TEXT              NOT NULL,
    submitted_at                  TIMESTAMP         NOT NULL,
    status                        submissionstatus  NOT NULL DEFAULT 'uploaded',
    ai_score                      DOUBLE PRECISION,
    final_score                   DOUBLE PRECISION,
    max_score                     DOUBLE PRECISION  NOT NULL DEFAULT 100,
    ai_analysis                   JSONB,
    ai_comments                   TEXT,
    ai_processed_at               TIMESTAMP,
    ai_request_started_at         TIMESTAMP,
    ai_request_completed_at       TIMESTAMP,
    ai_request_duration_seconds   DOUBLE PRECISION,
    ai_error                      TEXT,
    ai_retry_count                INTEGER,
    teacher_comments              TEXT,
    reviewed_by                   TEXT,
    reviewed_at                   TIMESTAMP,
    is_flagged                    BOOLEAN           NOT NULL DEFAULT FALSE,
    flag_reasons                  JSONB             NOT NULL DEFAULT '[]',
    anomaly_scores                JSONB             NOT NULL DEFAULT '{}',
    files_hash                    TEXT,
    created_at                    TIMESTAMP         NOT NULL DEFAULT now(),
    updated_at                    TIMESTAMP         NOT NULL DEFAULT now(),

    CONSTRAINT pk_submissions PRIMARY KEY (id),
    CONSTRAINT uq_submissions_session_id UNIQUE (session_id),
    CONSTRAINT fk_submissions_session
        FOREIGN KEY (session_id) REFERENCES exam_sessions (id) ON DELETE CASCADE,
    CONSTRAINT fk_submissions_student
        FOREIGN KEY (student_id) REFERENCES users (id),
    CONSTRAINT fk_submissions_reviewed_by
        FOREIGN KEY (reviewed_by) REFERENCES users (id)
);

-- 2.7 submission_images ---------------------------------------

CREATE TABLE submission_images (
    id               TEXT             NOT NULL,
    submission_id    TEXT             NOT NULL,
    filename         TEXT             NOT NULL,
    file_path        TEXT             NOT NULL,
    file_size        BIGINT           NOT NULL,
    mime_type        TEXT             NOT NULL,
    is_processed     BOOLEAN          NOT NULL DEFAULT FALSE,
    ocr_text         TEXT,
    quality_score    DOUBLE PRECISION,
    order_index      INTEGER          NOT NULL DEFAULT 0,
    perceptual_hash  TEXT,
    uploaded_at      TIMESTAMP        NOT NULL DEFAULT now(),
    processed_at     TIMESTAMP,

    CONSTRAINT pk_submission_images PRIMARY KEY (id),
    CONSTRAINT fk_submission_images_submission
        FOREIGN KEY (submission_id) REFERENCES submissions (id) ON DELETE CASCADE
);

-- 2.8 submission_scores ---------------------------------------

CREATE TABLE submission_scores (
    id                      TEXT             NOT NULL,
    submission_id           TEXT             NOT NULL,
    task_type_id            TEXT             NOT NULL,
    criterion_name          TEXT             NOT NULL,
    criterion_description   TEXT,
    ai_score                DOUBLE PRECISION,
    final_score             DOUBLE PRECISION,
    max_score               DOUBLE PRECISION NOT NULL DEFAULT 0,
    ai_comment              TEXT,
    teacher_comment         TEXT,
    created_at              TIMESTAMP        NOT NULL DEFAULT now(),
    updated_at              TIMESTAMP        NOT NULL DEFAULT now(),

    CONSTRAINT pk_submission_scores PRIMARY KEY (id),
    CONSTRAINT fk_submission_scores_submission
        FOREIGN KEY (submission_id) REFERENCES submissions (id) ON DELETE CASCADE,
    CONSTRAINT fk_submission_scores_task_type
        FOREIGN KEY (task_type_id) REFERENCES task_types (id) ON DELETE CASCADE
);

-- ------------------------------------------------------------
-- 3. Indexes
-- ------------------------------------------------------------

-- users -------------------------------------------------------
-- (isu already covered by UNIQUE constraint uq_users_isu)

-- exams -------------------------------------------------------
CREATE INDEX idx_exams_created_by     ON exams (created_by);
CREATE INDEX idx_exams_status         ON exams (status);
CREATE INDEX idx_exams_start_time     ON exams (start_time DESC);
CREATE INDEX idx_exams_status_end_time ON exams (status, end_time);

-- task_types --------------------------------------------------
CREATE INDEX idx_task_types_exam_id   ON task_types (exam_id);

-- task_variants -----------------------------------------------
CREATE INDEX idx_task_variants_task_type_id ON task_variants (task_type_id);

-- exam_sessions -----------------------------------------------
CREATE INDEX idx_exam_sessions_exam_id              ON exam_sessions (exam_id);
CREATE INDEX idx_exam_sessions_student_id           ON exam_sessions (student_id);
CREATE INDEX idx_exam_sessions_exam_student          ON exam_sessions (exam_id, student_id);
CREATE INDEX idx_exam_sessions_status               ON exam_sessions (status);
CREATE INDEX idx_exam_sessions_student_created       ON exam_sessions (student_id, created_at DESC);

-- submissions -------------------------------------------------
-- (session_id already covered by UNIQUE constraint uq_submissions_session_id)
CREATE INDEX idx_submissions_student_id              ON submissions (student_id);
CREATE INDEX idx_submissions_status                  ON submissions (status);
CREATE INDEX idx_submissions_status_ai_started       ON submissions (status, ai_request_started_at)
    WHERE ai_request_started_at IS NULL;
CREATE INDEX idx_submissions_flagged_retry           ON submissions (status, ai_retry_count)
    WHERE status = 'flagged' AND ai_error IS NOT NULL;
CREATE INDEX idx_submissions_created_at              ON submissions (created_at);

-- submission_images -------------------------------------------
CREATE INDEX idx_submission_images_submission_id     ON submission_images (submission_id);
CREATE INDEX idx_submission_images_order             ON submission_images (submission_id, order_index);

-- submission_scores -------------------------------------------
CREATE INDEX idx_submission_scores_submission_id     ON submission_scores (submission_id);
CREATE INDEX idx_submission_scores_task_type_id      ON submission_scores (task_type_id);
