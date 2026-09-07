ALTER TABLE task_bank_items
    ADD COLUMN solution TEXT,
    ADD COLUMN task_type TEXT,
    ADD COLUMN difficulty TEXT,
    ADD COLUMN volume TEXT;

CREATE INDEX idx_task_bank_classification
    ON task_bank_items (source_id, task_type, difficulty, volume);
CREATE INDEX idx_task_bank_solution
    ON task_bank_items (source_id) WHERE solution IS NOT NULL;
