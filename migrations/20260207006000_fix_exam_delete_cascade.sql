-- Ensure task_types FK cascades on exam delete.
-- Production DB may have been created before CASCADE or with a different schema.
-- Without CASCADE, DELETE FROM exams fails with "violates foreign key constraint task_types_exam_id_fkey".

ALTER TABLE task_types
    DROP CONSTRAINT IF EXISTS task_types_exam_id_fkey;

ALTER TABLE task_types
    ADD CONSTRAINT task_types_exam_id_fkey
    FOREIGN KEY (exam_id) REFERENCES exams(id) ON DELETE CASCADE;
