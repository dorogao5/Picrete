-- Convert existing enum values from UPPERCASE to lowercase.
-- Run AFTER 20260207004000 (which adds lowercase values to enums).
-- Existing rows may have been inserted with 'DRAFT', 'MEDIUM' etc.
-- Rust/sqlx expects lowercase; deserialization fails on uppercase.

UPDATE exams SET status = lower(status::text)::examstatus WHERE status::text <> lower(status::text);
UPDATE task_types SET difficulty = lower(difficulty::text)::difficultylevel WHERE difficulty::text <> lower(difficulty::text);
UPDATE exam_sessions SET status = lower(status::text)::sessionstatus WHERE status::text <> lower(status::text);
UPDATE submissions SET status = lower(status::text)::submissionstatus WHERE status::text <> lower(status::text);
UPDATE users SET role = lower(role::text)::userrole WHERE role::text <> lower(role::text);
