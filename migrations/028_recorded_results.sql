-- A report from record_results names only the tests that it ran. Its skip and todo
-- results count, and an omitted test gets no result.
ALTER TABLE joppa.test_reports ADD COLUMN IF NOT EXISTS recorded boolean NOT NULL DEFAULT false;
