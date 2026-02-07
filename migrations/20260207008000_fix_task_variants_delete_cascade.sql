-- Ensure FKs cascade when task_types are deleted (as part of exam delete).
-- Without CASCADE, deleting task_types fails with FK violations.

-- task_variants
ALTER TABLE task_variants
    DROP CONSTRAINT IF EXISTS task_variants_task_type_id_fkey;

ALTER TABLE task_variants
    ADD CONSTRAINT task_variants_task_type_id_fkey
    FOREIGN KEY (task_type_id) REFERENCES task_types(id) ON DELETE CASCADE;

-- submission_scores (also references task_types)
ALTER TABLE submission_scores
    DROP CONSTRAINT IF EXISTS submission_scores_task_type_id_fkey;

ALTER TABLE submission_scores
    ADD CONSTRAINT submission_scores_task_type_id_fkey
    FOREIGN KEY (task_type_id) REFERENCES task_types(id) ON DELETE CASCADE;
