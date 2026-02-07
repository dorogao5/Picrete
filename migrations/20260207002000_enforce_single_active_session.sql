CREATE UNIQUE INDEX IF NOT EXISTS ux_exam_sessions_active_student_exam
    ON exam_sessions (exam_id, student_id)
    WHERE status = 'active';
