-- A test result keeps the status name and the extra fields of the schema result shape.
-- A result without them keeps NULL. The rows stay append-only.
ALTER TABLE joppa.test_results ADD COLUMN IF NOT EXISTS status_name text;
ALTER TABLE joppa.test_results ADD COLUMN IF NOT EXISTS fields jsonb;
