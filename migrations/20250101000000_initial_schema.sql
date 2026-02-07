-- Initial schema for Picrete

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'userrole') THEN
        CREATE TYPE userrole AS ENUM ('admin', 'teacher', 'assistant', 'student');
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'examstatus') THEN
        CREATE TYPE examstatus AS ENUM ('draft', 'published', 'active', 'completed', 'archived');
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'difficultylevel') THEN
        CREATE TYPE difficultylevel AS ENUM ('easy', 'medium', 'hard');
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'sessionstatus') THEN
        CREATE TYPE sessionstatus AS ENUM ('active', 'submitted', 'expired', 'graded');
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'submissionstatus') THEN
        CREATE TYPE submissionstatus AS ENUM ('uploaded', 'processing', 'preliminary', 'approved', 'flagged', 'rejected');
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    isu VARCHAR(6) NOT NULL UNIQUE,
    hashed_password TEXT NOT NULL,
    full_name TEXT NOT NULL,
    role userrole NOT NULL DEFAULT 'student',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    is_verified BOOLEAN NOT NULL DEFAULT FALSE,
    pd_consent BOOLEAN NOT NULL DEFAULT FALSE,
    pd_consent_at TIMESTAMPTZ NULL,
    pd_consent_version TEXT NULL,
    terms_accepted_at TIMESTAMPTZ NULL,
    terms_version TEXT NULL,
    privacy_version TEXT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    updated_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
);

CREATE TABLE IF NOT EXISTS exams (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT NULL,
    start_time TIMESTAMP NOT NULL,
    end_time TIMESTAMP NOT NULL,
    duration_minutes INTEGER NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'Europe/Moscow',
    max_attempts INTEGER NOT NULL DEFAULT 1,
    allow_breaks BOOLEAN NOT NULL DEFAULT FALSE,
    break_duration_minutes INTEGER NOT NULL DEFAULT 0,
    auto_save_interval INTEGER NOT NULL DEFAULT 10,
    status examstatus NOT NULL DEFAULT 'draft',
    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    updated_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    published_at TIMESTAMP NULL,
    settings JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS task_types (
    id TEXT PRIMARY KEY,
    exam_id TEXT NOT NULL REFERENCES exams(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    order_index INTEGER NOT NULL,
    max_score DOUBLE PRECISION NOT NULL,
    rubric JSONB NOT NULL,
    difficulty difficultylevel NOT NULL DEFAULT 'medium',
    taxonomy_tags JSONB NOT NULL DEFAULT '[]'::jsonb,
    formulas JSONB NOT NULL DEFAULT '[]'::jsonb,
    units JSONB NOT NULL DEFAULT '[]'::jsonb,
    validation_rules JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    updated_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
);

CREATE TABLE IF NOT EXISTS task_variants (
    id TEXT PRIMARY KEY,
    task_type_id TEXT NOT NULL REFERENCES task_types(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    parameters JSONB NOT NULL DEFAULT '{}'::jsonb,
    reference_solution TEXT NULL,
    reference_answer TEXT NULL,
    answer_tolerance DOUBLE PRECISION NOT NULL DEFAULT 0.01,
    attachments JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
);

CREATE TABLE IF NOT EXISTS exam_sessions (
    id TEXT PRIMARY KEY,
    exam_id TEXT NOT NULL REFERENCES exams(id) ON DELETE CASCADE,
    student_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    variant_seed INTEGER NOT NULL,
    variant_assignments JSONB NOT NULL DEFAULT '{}'::jsonb,
    started_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    submitted_at TIMESTAMP NULL,
    expires_at TIMESTAMP NOT NULL,
    status sessionstatus NOT NULL DEFAULT 'active',
    attempt_number INTEGER NOT NULL DEFAULT 1,
    ip_address TEXT NULL,
    user_agent TEXT NULL,
    last_auto_save TIMESTAMP NULL,
    auto_save_data JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    updated_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
);

CREATE TABLE IF NOT EXISTS submissions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL UNIQUE REFERENCES exam_sessions(id) ON DELETE CASCADE,
    student_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    submitted_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    status submissionstatus NOT NULL DEFAULT 'uploaded',
    ai_score DOUBLE PRECISION NULL,
    final_score DOUBLE PRECISION NULL,
    max_score DOUBLE PRECISION NOT NULL,
    ai_analysis JSONB NULL,
    ai_comments TEXT NULL,
    ai_processed_at TIMESTAMP NULL,
    ai_request_started_at TIMESTAMP NULL,
    ai_request_completed_at TIMESTAMP NULL,
    ai_request_duration_seconds DOUBLE PRECISION NULL,
    ai_error TEXT NULL,
    ai_retry_count INTEGER NOT NULL DEFAULT 0,
    teacher_comments TEXT NULL,
    reviewed_by TEXT NULL REFERENCES users(id),
    reviewed_at TIMESTAMP NULL,
    is_flagged BOOLEAN NOT NULL DEFAULT FALSE,
    flag_reasons JSONB NOT NULL DEFAULT '[]'::jsonb,
    anomaly_scores JSONB NOT NULL DEFAULT '{}'::jsonb,
    files_hash TEXT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    updated_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
);

CREATE TABLE IF NOT EXISTS submission_images (
    id TEXT PRIMARY KEY,
    submission_id TEXT NOT NULL REFERENCES submissions(id) ON DELETE CASCADE,
    filename TEXT NOT NULL,
    file_path TEXT NOT NULL,
    file_size BIGINT NOT NULL,
    mime_type TEXT NOT NULL,
    is_processed BOOLEAN NOT NULL DEFAULT FALSE,
    ocr_text TEXT NULL,
    quality_score DOUBLE PRECISION NULL,
    order_index INTEGER NOT NULL,
    perceptual_hash TEXT NULL,
    uploaded_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    processed_at TIMESTAMP NULL
);

CREATE TABLE IF NOT EXISTS submission_scores (
    id TEXT PRIMARY KEY,
    submission_id TEXT NOT NULL REFERENCES submissions(id) ON DELETE CASCADE,
    task_type_id TEXT NOT NULL REFERENCES task_types(id) ON DELETE CASCADE,
    criterion_name TEXT NOT NULL,
    criterion_description TEXT NULL,
    ai_score DOUBLE PRECISION NULL,
    final_score DOUBLE PRECISION NULL,
    max_score DOUBLE PRECISION NOT NULL,
    ai_comment TEXT NULL,
    teacher_comment TEXT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
    updated_at TIMESTAMP NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
);
