-- Fix NULL auto_save_data in exam_sessions (causes "unexpected null" when student enters exam).
-- Schema says NOT NULL DEFAULT '{}', but some rows may have NULL (e.g. from older inserts).

UPDATE exam_sessions SET auto_save_data = '{}'::jsonb WHERE auto_save_data IS NULL;
