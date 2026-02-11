DO $$
BEGIN
    CREATE TYPE uploadsource AS ENUM ('web', 'telegram');
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

ALTER TABLE submission_images
    ADD COLUMN IF NOT EXISTS upload_source uploadsource NOT NULL DEFAULT 'web';

CREATE INDEX IF NOT EXISTS idx_submission_images_course_submission_uploaded_at
    ON submission_images (course_id, submission_id, uploaded_at);
