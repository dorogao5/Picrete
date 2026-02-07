CREATE INDEX IF NOT EXISTS idx_submissions_status_started
    ON submissions (status, ai_request_started_at);

CREATE INDEX IF NOT EXISTS idx_exam_sessions_exam_student
    ON exam_sessions (exam_id, student_id);

CREATE INDEX IF NOT EXISTS idx_exam_sessions_status
    ON exam_sessions (status);

CREATE INDEX IF NOT EXISTS idx_submission_images_submission
    ON submission_images (submission_id);

CREATE INDEX IF NOT EXISTS idx_exams_status_end_time
    ON exams (status, end_time);

CREATE INDEX IF NOT EXISTS idx_submissions_student
    ON submissions (student_id);

CREATE INDEX IF NOT EXISTS idx_submission_scores_submission
    ON submission_scores (submission_id);
