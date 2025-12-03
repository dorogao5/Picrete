-- Migration: Add AI logging fields to submissions table
-- Description: Add fields to track AI request timing, errors, and retry count
-- Date: 2025-10-06

-- Add new columns for AI request tracking
ALTER TABLE submissions
ADD COLUMN IF NOT EXISTS ai_request_started_at TIMESTAMP,
ADD COLUMN IF NOT EXISTS ai_request_completed_at TIMESTAMP,
ADD COLUMN IF NOT EXISTS ai_request_duration_seconds DOUBLE PRECISION,
ADD COLUMN IF NOT EXISTS ai_error TEXT,
ADD COLUMN IF NOT EXISTS ai_retry_count INTEGER DEFAULT 0;

-- Add index on ai_request_started_at for querying processing times
CREATE INDEX IF NOT EXISTS idx_submissions_ai_request_started 
ON submissions(ai_request_started_at);

-- Add index on status for filtering submissions by status
CREATE INDEX IF NOT EXISTS idx_submissions_status 
ON submissions(status);

-- Add index on ai_processed_at for querying completed AI processing
CREATE INDEX IF NOT EXISTS idx_submissions_ai_processed_at 
ON submissions(ai_processed_at);

-- Update existing submissions to have default retry count
UPDATE submissions 
SET ai_retry_count = 0 
WHERE ai_retry_count IS NULL;

-- Add comment to table
COMMENT ON COLUMN submissions.ai_request_started_at IS 'Timestamp when AI grading request was initiated';
COMMENT ON COLUMN submissions.ai_request_completed_at IS 'Timestamp when AI grading request completed (success or failure)';
COMMENT ON COLUMN submissions.ai_request_duration_seconds IS 'Total duration of AI request in seconds';
COMMENT ON COLUMN submissions.ai_error IS 'Error message if AI processing failed';
COMMENT ON COLUMN submissions.ai_retry_count IS 'Number of times AI grading was retried for this submission';

