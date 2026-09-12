-- Authored resources are shared; attempts and conversations are private.
CREATE TABLE course_trainers (
 id TEXT PRIMARY KEY, course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
 author_id TEXT NOT NULL REFERENCES users(id), draft JSONB NOT NULL,
 published JSONB, release_id TEXT, revision INTEGER NOT NULL DEFAULT 1,
 updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), UNIQUE(id,course_id)
);
CREATE TABLE practice_attempts (
 id TEXT PRIMARY KEY, course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
 student_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 trainer_id TEXT REFERENCES course_trainers(id), release_id TEXT, section_id TEXT,
 set_id TEXT REFERENCES trainer_sets(id) ON DELETE SET NULL, difficulty TEXT NOT NULL,
 task_id TEXT NOT NULL REFERENCES task_bank_items(id), task JSONB NOT NULL,
 snapshot JSONB NOT NULL, title TEXT NOT NULL, preview BOOLEAN NOT NULL DEFAULT FALSE,
 draft TEXT NOT NULL DEFAULT '', revision INTEGER NOT NULL DEFAULT 0,
 messages JSONB NOT NULL DEFAULT '[]', checks JSONB NOT NULL DEFAULT '[]',
 helped BOOLEAN NOT NULL DEFAULT FALSE, revealed BOOLEAN NOT NULL DEFAULT FALSE,
 solved BOOLEAN NOT NULL DEFAULT FALSE, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), UNIQUE(id,course_id)
);
CREATE INDEX practice_owner ON practice_attempts(course_id,student_id,updated_at DESC);
CREATE TABLE practice_jobs (
 id TEXT PRIMARY KEY, attempt_id TEXT NOT NULL REFERENCES practice_attempts(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('message','check','ocr')),
 payload JSONB NOT NULL, revision INTEGER NOT NULL,
 status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','running','completed','failed')),
 error TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), started_at TIMESTAMPTZ
);
CREATE UNIQUE INDEX practice_one_job ON practice_jobs(attempt_id) WHERE status IN ('queued','running');
CREATE INDEX practice_queue ON practice_jobs(created_at) WHERE status='queued';
CREATE TABLE practice_photos (
 id TEXT PRIMARY KEY, attempt_id TEXT NOT NULL REFERENCES practice_attempts(id) ON DELETE CASCADE,
 storage_key TEXT NOT NULL, filename TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
