-- Ensure exam_sessions FK cascades on exam delete.
-- Production DB may have been created without CASCADE.
-- Without CASCADE, DELETE FROM exams fails with "violates foreign key constraint exam_sessions_exam_id_fkey".

ALTER TABLE exam_sessions
    DROP CONSTRAINT IF EXISTS exam_sessions_exam_id_fkey;

ALTER TABLE exam_sessions
    ADD CONSTRAINT exam_sessions_exam_id_fkey
    FOREIGN KEY (exam_id) REFERENCES exams(id) ON DELETE CASCADE;
