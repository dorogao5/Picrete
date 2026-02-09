-- ============================================================
-- Picrete — initial database schema (multi-course baseline)
-- ============================================================
-- Run against a fresh PostgreSQL 15+ database.
-- All identifiers are lower-case to match sqlx type_name attributes.
-- ============================================================

-- ------------------------------------------------------------
-- 1. Custom ENUM types
-- ------------------------------------------------------------

CREATE TYPE courserole AS ENUM (
    'teacher',
    'student'
);

CREATE TYPE membershipstatus AS ENUM (
    'active',
    'suspended',
    'left'
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

CREATE TYPE ocrimagestatus AS ENUM (
    'pending',
    'processing',
    'ready',
    'failed'
);

CREATE TYPE ocroverallstatus AS ENUM (
    'not_required',
    'pending',
    'processing',
    'in_review',
    'validated',
    'reported',
    'failed'
);

CREATE TYPE llmprecheckstatus AS ENUM (
    'skipped',
    'queued',
    'processing',
    'completed',
    'failed'
);

CREATE TYPE ocrpagestatus AS ENUM (
    'approved',
    'reported'
);

CREATE TYPE ocrissueseverity AS ENUM (
    'minor',
    'major',
    'critical'
);

-- ------------------------------------------------------------
-- 2. Core identity and course domain
-- ------------------------------------------------------------

CREATE TABLE users (
    id                  TEXT        NOT NULL,
    username            TEXT        NOT NULL,
    hashed_password     TEXT        NOT NULL,
    full_name           TEXT        NOT NULL,
    is_platform_admin   BOOLEAN     NOT NULL DEFAULT FALSE,
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
    CONSTRAINT uq_users_username UNIQUE (username)
);

CREATE TABLE courses (
    id            TEXT       NOT NULL,
    slug          TEXT       NOT NULL,
    title         TEXT       NOT NULL,
    organization  TEXT,
    is_active     BOOLEAN    NOT NULL DEFAULT TRUE,
    created_by    TEXT       NOT NULL,
    created_at    TIMESTAMP  NOT NULL DEFAULT now(),
    updated_at    TIMESTAMP  NOT NULL DEFAULT now(),

    CONSTRAINT pk_courses PRIMARY KEY (id),
    CONSTRAINT uq_courses_slug UNIQUE (slug),
    CONSTRAINT fk_courses_created_by
        FOREIGN KEY (created_by) REFERENCES users (id) ON DELETE RESTRICT
);

CREATE TABLE course_memberships (
    id                TEXT              NOT NULL,
    course_id         TEXT              NOT NULL,
    user_id           TEXT              NOT NULL,
    status            membershipstatus  NOT NULL DEFAULT 'active',
    joined_at         TIMESTAMP         NOT NULL DEFAULT now(),
    invited_by        TEXT,
    identity_payload  JSONB             NOT NULL DEFAULT '{}',

    CONSTRAINT pk_course_memberships PRIMARY KEY (id),
    CONSTRAINT uq_course_memberships_course_user UNIQUE (course_id, user_id),
    CONSTRAINT fk_course_memberships_course
        FOREIGN KEY (course_id) REFERENCES courses (id) ON DELETE CASCADE,
    CONSTRAINT fk_course_memberships_user
        FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE,
    CONSTRAINT fk_course_memberships_invited_by
        FOREIGN KEY (invited_by) REFERENCES users (id) ON DELETE SET NULL
);

CREATE TABLE course_membership_roles (
    membership_id  TEXT       NOT NULL,
    role           courserole NOT NULL,
    granted_at     TIMESTAMP  NOT NULL DEFAULT now(),

    CONSTRAINT pk_course_membership_roles PRIMARY KEY (membership_id, role),
    CONSTRAINT fk_course_membership_roles_membership
        FOREIGN KEY (membership_id) REFERENCES course_memberships (id) ON DELETE CASCADE
);

CREATE TABLE course_invite_codes (
    id               TEXT       NOT NULL,
    course_id        TEXT       NOT NULL,
    role             courserole NOT NULL,
    code_hash        TEXT       NOT NULL,
    is_active        BOOLEAN    NOT NULL DEFAULT TRUE,
    rotated_from_id  TEXT,
    expires_at       TIMESTAMP,
    usage_count      BIGINT     NOT NULL DEFAULT 0,
    created_at       TIMESTAMP  NOT NULL DEFAULT now(),
    updated_at       TIMESTAMP  NOT NULL DEFAULT now(),

    CONSTRAINT pk_course_invite_codes PRIMARY KEY (id),
    CONSTRAINT fk_course_invite_codes_course
        FOREIGN KEY (course_id) REFERENCES courses (id) ON DELETE CASCADE,
    CONSTRAINT fk_course_invite_codes_rotated_from
        FOREIGN KEY (rotated_from_id) REFERENCES course_invite_codes (id) ON DELETE SET NULL
);

CREATE TABLE course_identity_policies (
    course_id    TEXT       NOT NULL,
    rule_type    TEXT       NOT NULL,
    rule_config  JSONB      NOT NULL DEFAULT '{}',
    updated_at   TIMESTAMP  NOT NULL DEFAULT now(),

    CONSTRAINT pk_course_identity_policies PRIMARY KEY (course_id),
    CONSTRAINT fk_course_identity_policies_course
        FOREIGN KEY (course_id) REFERENCES courses (id) ON DELETE CASCADE
);

-- ------------------------------------------------------------
-- 3. Academic domain (strictly course-scoped)
-- ------------------------------------------------------------

CREATE TABLE exams (
    id                      TEXT        NOT NULL,
    course_id               TEXT        NOT NULL,
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
    created_by              TEXT,
    created_at              TIMESTAMP   NOT NULL DEFAULT now(),
    updated_at              TIMESTAMP   NOT NULL DEFAULT now(),
    published_at            TIMESTAMP,
    settings                JSONB       NOT NULL DEFAULT '{}',

    CONSTRAINT pk_exams PRIMARY KEY (id),
    CONSTRAINT uq_exams_id_course UNIQUE (id, course_id),
    CONSTRAINT fk_exams_course
        FOREIGN KEY (course_id) REFERENCES courses (id) ON DELETE CASCADE,
    CONSTRAINT fk_exams_created_by
        FOREIGN KEY (created_by) REFERENCES users (id) ON DELETE SET NULL
);

CREATE TABLE task_types (
    id                TEXT             NOT NULL,
    course_id         TEXT             NOT NULL,
    exam_id           TEXT             NOT NULL,
    title             TEXT             NOT NULL,
    description       TEXT             NOT NULL,
    order_index       INTEGER          NOT NULL DEFAULT 0,
    max_score         DOUBLE PRECISION NOT NULL DEFAULT 0,
    rubric            JSONB            NOT NULL DEFAULT '{}',
    difficulty        difficultylevel  NOT NULL DEFAULT 'medium',
    taxonomy_tags     JSONB            NOT NULL DEFAULT '[]',
    formulas          JSONB            NOT NULL DEFAULT '[]',
    units             JSONB            NOT NULL DEFAULT '[]',
    validation_rules  JSONB            NOT NULL DEFAULT '{}',
    created_at        TIMESTAMP        NOT NULL DEFAULT now(),
    updated_at        TIMESTAMP        NOT NULL DEFAULT now(),

    CONSTRAINT pk_task_types PRIMARY KEY (id),
    CONSTRAINT uq_task_types_id_course UNIQUE (id, course_id),
    CONSTRAINT fk_task_types_exam
        FOREIGN KEY (exam_id, course_id) REFERENCES exams (id, course_id) ON DELETE CASCADE
);

CREATE TABLE task_variants (
    id                  TEXT             NOT NULL,
    course_id           TEXT             NOT NULL,
    task_type_id        TEXT             NOT NULL,
    content             TEXT             NOT NULL,
    parameters          JSONB            NOT NULL DEFAULT '{}',
    reference_solution  TEXT,
    reference_answer    TEXT,
    answer_tolerance    DOUBLE PRECISION NOT NULL DEFAULT 0,
    attachments         JSONB            NOT NULL DEFAULT '[]',
    created_at          TIMESTAMP        NOT NULL DEFAULT now(),

    CONSTRAINT pk_task_variants PRIMARY KEY (id),
    CONSTRAINT uq_task_variants_id_course UNIQUE (id, course_id),
    CONSTRAINT fk_task_variants_task_type
        FOREIGN KEY (task_type_id, course_id) REFERENCES task_types (id, course_id) ON DELETE CASCADE
);

CREATE TABLE exam_sessions (
    id                    TEXT           NOT NULL,
    course_id             TEXT           NOT NULL,
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
    CONSTRAINT uq_exam_sessions_id_course UNIQUE (id, course_id),
    CONSTRAINT fk_exam_sessions_exam
        FOREIGN KEY (exam_id, course_id) REFERENCES exams (id, course_id) ON DELETE CASCADE,
    CONSTRAINT fk_exam_sessions_student
        FOREIGN KEY (student_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE TABLE submissions (
    id                            TEXT              NOT NULL,
    course_id                     TEXT              NOT NULL,
    session_id                    TEXT              NOT NULL,
    student_id                    TEXT              NOT NULL,
    submitted_at                  TIMESTAMP         NOT NULL,
    status                        submissionstatus  NOT NULL DEFAULT 'uploaded',
    ocr_overall_status            ocroverallstatus  NOT NULL DEFAULT 'pending',
    llm_precheck_status           llmprecheckstatus NOT NULL DEFAULT 'skipped',
    report_flag                   BOOLEAN           NOT NULL DEFAULT FALSE,
    report_summary                TEXT,
    ai_score                      DOUBLE PRECISION,
    final_score                   DOUBLE PRECISION,
    max_score                     DOUBLE PRECISION  NOT NULL DEFAULT 100,
    ai_analysis                   JSONB,
    ai_comments                   TEXT,
    ocr_error                     TEXT,
    ocr_retry_count               INTEGER           NOT NULL DEFAULT 0,
    ocr_started_at                TIMESTAMP,
    ocr_completed_at              TIMESTAMP,
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
    CONSTRAINT uq_submissions_id_course UNIQUE (id, course_id),
    CONSTRAINT uq_submissions_session_id UNIQUE (session_id),
    CONSTRAINT fk_submissions_session
        FOREIGN KEY (session_id, course_id) REFERENCES exam_sessions (id, course_id) ON DELETE CASCADE,
    CONSTRAINT fk_submissions_student
        FOREIGN KEY (student_id) REFERENCES users (id) ON DELETE CASCADE,
    CONSTRAINT fk_submissions_reviewed_by
        FOREIGN KEY (reviewed_by) REFERENCES users (id) ON DELETE SET NULL
);

CREATE TABLE submission_images (
    id               TEXT             NOT NULL,
    course_id        TEXT             NOT NULL,
    submission_id    TEXT             NOT NULL,
    filename         TEXT             NOT NULL,
    file_path        TEXT             NOT NULL,
    file_size        BIGINT           NOT NULL,
    mime_type        TEXT             NOT NULL,
    is_processed     BOOLEAN          NOT NULL DEFAULT FALSE,
    ocr_status       ocrimagestatus   NOT NULL DEFAULT 'pending',
    ocr_text         TEXT,
    ocr_markdown     TEXT,
    ocr_chunks       JSONB,
    ocr_model        TEXT,
    ocr_completed_at TIMESTAMP,
    ocr_error        TEXT,
    ocr_request_id   TEXT,
    quality_score    DOUBLE PRECISION,
    order_index      INTEGER          NOT NULL DEFAULT 0,
    perceptual_hash  TEXT,
    uploaded_at      TIMESTAMP        NOT NULL DEFAULT now(),
    processed_at     TIMESTAMP,

    CONSTRAINT pk_submission_images PRIMARY KEY (id),
    CONSTRAINT uq_submission_images_id_course UNIQUE (id, course_id),
    CONSTRAINT fk_submission_images_submission
        FOREIGN KEY (submission_id, course_id) REFERENCES submissions (id, course_id) ON DELETE CASCADE
);

CREATE TABLE submission_ocr_reviews (
    id            TEXT           NOT NULL,
    course_id     TEXT           NOT NULL,
    submission_id TEXT           NOT NULL,
    image_id      TEXT           NOT NULL,
    student_id    TEXT           NOT NULL,
    page_status   ocrpagestatus  NOT NULL,
    issue_count   INTEGER        NOT NULL DEFAULT 0,
    created_at    TIMESTAMP      NOT NULL DEFAULT now(),
    updated_at    TIMESTAMP      NOT NULL DEFAULT now(),

    CONSTRAINT pk_submission_ocr_reviews PRIMARY KEY (id),
    CONSTRAINT uq_submission_ocr_reviews_id_course UNIQUE (id, course_id),
    CONSTRAINT uq_submission_ocr_reviews_submission_image UNIQUE (submission_id, image_id),
    CONSTRAINT fk_submission_ocr_reviews_submission
        FOREIGN KEY (submission_id, course_id) REFERENCES submissions (id, course_id) ON DELETE CASCADE,
    CONSTRAINT fk_submission_ocr_reviews_image
        FOREIGN KEY (image_id, course_id) REFERENCES submission_images (id, course_id) ON DELETE CASCADE,
    CONSTRAINT fk_submission_ocr_reviews_student
        FOREIGN KEY (student_id) REFERENCES users (id) ON DELETE CASCADE
);

CREATE TABLE submission_ocr_issues (
    id              TEXT              NOT NULL,
    course_id       TEXT              NOT NULL,
    ocr_review_id   TEXT              NOT NULL,
    submission_id   TEXT              NOT NULL,
    image_id        TEXT              NOT NULL,
    anchor          JSONB             NOT NULL,
    original_text   TEXT,
    suggested_text  TEXT,
    note            TEXT              NOT NULL,
    severity        ocrissueseverity  NOT NULL DEFAULT 'major',
    created_at      TIMESTAMP         NOT NULL DEFAULT now(),
    updated_at      TIMESTAMP         NOT NULL DEFAULT now(),

    CONSTRAINT pk_submission_ocr_issues PRIMARY KEY (id),
    CONSTRAINT fk_submission_ocr_issues_review
        FOREIGN KEY (ocr_review_id, course_id) REFERENCES submission_ocr_reviews (id, course_id) ON DELETE CASCADE,
    CONSTRAINT fk_submission_ocr_issues_submission
        FOREIGN KEY (submission_id, course_id) REFERENCES submissions (id, course_id) ON DELETE CASCADE,
    CONSTRAINT fk_submission_ocr_issues_image
        FOREIGN KEY (image_id, course_id) REFERENCES submission_images (id, course_id) ON DELETE CASCADE
);

CREATE TABLE submission_scores (
    id                      TEXT             NOT NULL,
    course_id               TEXT             NOT NULL,
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
        FOREIGN KEY (submission_id, course_id) REFERENCES submissions (id, course_id) ON DELETE CASCADE,
    CONSTRAINT fk_submission_scores_task_type
        FOREIGN KEY (task_type_id, course_id) REFERENCES task_types (id, course_id) ON DELETE CASCADE
);

-- ------------------------------------------------------------
-- 4. Indexes
-- ------------------------------------------------------------

-- users -------------------------------------------------------
CREATE INDEX idx_users_created_at ON users (created_at DESC);

-- courses -----------------------------------------------------
CREATE INDEX idx_courses_created_by ON courses (created_by);

-- course memberships -----------------------------------------
CREATE INDEX idx_course_memberships_user_id ON course_memberships (user_id);
CREATE INDEX idx_course_memberships_course_status ON course_memberships (course_id, status);

-- invite codes ------------------------------------------------
CREATE UNIQUE INDEX uq_course_invite_codes_code_hash ON course_invite_codes (code_hash);
CREATE UNIQUE INDEX uq_course_invite_codes_active_role
    ON course_invite_codes (course_id, role)
    WHERE is_active = TRUE;
CREATE INDEX idx_course_invite_codes_course_active ON course_invite_codes (course_id, is_active);

-- exams -------------------------------------------------------
CREATE INDEX idx_exams_course_status ON exams (course_id, status);
CREATE INDEX idx_exams_course_start_time ON exams (course_id, start_time DESC);
CREATE INDEX idx_exams_course_status_end_time ON exams (course_id, status, end_time);

-- task types / variants --------------------------------------
CREATE INDEX idx_task_types_course_exam ON task_types (course_id, exam_id, order_index);
CREATE INDEX idx_task_variants_course_task_type ON task_variants (course_id, task_type_id);

-- exam sessions ----------------------------------------------
CREATE INDEX idx_exam_sessions_course_exam_student ON exam_sessions (course_id, exam_id, student_id);
CREATE INDEX idx_exam_sessions_course_student_created ON exam_sessions (course_id, student_id, created_at DESC);
CREATE INDEX idx_exam_sessions_course_status ON exam_sessions (course_id, status);

-- submissions -------------------------------------------------
CREATE INDEX idx_submissions_course_student ON submissions (course_id, student_id);
CREATE INDEX idx_submissions_course_status ON submissions (course_id, status);
CREATE INDEX idx_submissions_course_ocr_status ON submissions (course_id, ocr_overall_status);
CREATE INDEX idx_submissions_course_llm_status ON submissions (course_id, llm_precheck_status);
CREATE INDEX idx_submissions_course_status_ai_started
    ON submissions (course_id, status, ai_request_started_at)
    WHERE ai_request_started_at IS NULL;
CREATE INDEX idx_submissions_course_flagged_retry
    ON submissions (course_id, status, ai_retry_count)
    WHERE status = 'flagged' AND ai_error IS NOT NULL;
CREATE INDEX idx_submissions_course_ocr_failed_retry
    ON submissions (course_id, ocr_overall_status, ocr_retry_count)
    WHERE ocr_overall_status = 'failed';
CREATE INDEX idx_submissions_course_created_at ON submissions (course_id, created_at);

-- submission images -------------------------------------------
CREATE INDEX idx_submission_images_course_submission ON submission_images (course_id, submission_id);
CREATE INDEX idx_submission_images_course_submission_order
    ON submission_images (course_id, submission_id, order_index);
CREATE INDEX idx_submission_images_course_submission_ocr
    ON submission_images (course_id, submission_id, ocr_status);
CREATE INDEX idx_submission_images_course_ocr_request_id
    ON submission_images (course_id, ocr_request_id);

-- submission ocr reviews / issues -----------------------------
CREATE INDEX idx_submission_ocr_reviews_course_submission
    ON submission_ocr_reviews (course_id, submission_id);
CREATE INDEX idx_submission_ocr_reviews_course_student
    ON submission_ocr_reviews (course_id, student_id);
CREATE INDEX idx_submission_ocr_issues_course_submission
    ON submission_ocr_issues (course_id, submission_id);
CREATE INDEX idx_submission_ocr_issues_course_review
    ON submission_ocr_issues (course_id, ocr_review_id);

-- submission scores -------------------------------------------
CREATE INDEX idx_submission_scores_course_submission ON submission_scores (course_id, submission_id);
CREATE INDEX idx_submission_scores_course_task_type ON submission_scores (course_id, task_type_id);
