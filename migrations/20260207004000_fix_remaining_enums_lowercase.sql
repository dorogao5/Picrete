-- Add lowercase values to remaining enums. Rust/sqlx uses rename_all = "lowercase".
-- Production DB may have uppercase-only values. See 20260207003000 for difficultylevel.
-- IF NOT EXISTS prevents errors if value already exists (PostgreSQL 9.1+).

-- userrole
ALTER TYPE userrole ADD VALUE IF NOT EXISTS 'admin';
ALTER TYPE userrole ADD VALUE IF NOT EXISTS 'teacher';
ALTER TYPE userrole ADD VALUE IF NOT EXISTS 'assistant';
ALTER TYPE userrole ADD VALUE IF NOT EXISTS 'student';

-- examstatus
ALTER TYPE examstatus ADD VALUE IF NOT EXISTS 'draft';
ALTER TYPE examstatus ADD VALUE IF NOT EXISTS 'published';
ALTER TYPE examstatus ADD VALUE IF NOT EXISTS 'active';
ALTER TYPE examstatus ADD VALUE IF NOT EXISTS 'completed';
ALTER TYPE examstatus ADD VALUE IF NOT EXISTS 'archived';

-- sessionstatus
ALTER TYPE sessionstatus ADD VALUE IF NOT EXISTS 'active';
ALTER TYPE sessionstatus ADD VALUE IF NOT EXISTS 'submitted';
ALTER TYPE sessionstatus ADD VALUE IF NOT EXISTS 'expired';
ALTER TYPE sessionstatus ADD VALUE IF NOT EXISTS 'graded';

-- submissionstatus
ALTER TYPE submissionstatus ADD VALUE IF NOT EXISTS 'uploaded';
ALTER TYPE submissionstatus ADD VALUE IF NOT EXISTS 'processing';
ALTER TYPE submissionstatus ADD VALUE IF NOT EXISTS 'preliminary';
ALTER TYPE submissionstatus ADD VALUE IF NOT EXISTS 'approved';
ALTER TYPE submissionstatus ADD VALUE IF NOT EXISTS 'flagged';
ALTER TYPE submissionstatus ADD VALUE IF NOT EXISTS 'rejected';
