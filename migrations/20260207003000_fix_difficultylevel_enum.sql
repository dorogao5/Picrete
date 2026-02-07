-- Ensure difficultylevel enum has 'easy', 'medium', 'hard' (lowercase).
-- Production DB may have been created with different schema (e.g. only easy/hard,
-- or different casing). This migration adds missing values.
-- IF NOT EXISTS prevents errors if value already exists (PostgreSQL 9.1+).

ALTER TYPE difficultylevel ADD VALUE IF NOT EXISTS 'easy';
ALTER TYPE difficultylevel ADD VALUE IF NOT EXISTS 'medium';
ALTER TYPE difficultylevel ADD VALUE IF NOT EXISTS 'hard';
