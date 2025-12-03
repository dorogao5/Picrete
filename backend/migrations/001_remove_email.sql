-- Migration: Remove email field from users table
-- Date: 2025-10-03
-- Description: Remove email column as system now uses only ISU numbers

-- Remove email column
ALTER TABLE users DROP COLUMN IF EXISTS email;

-- Note: The unique constraint on email will be automatically dropped

