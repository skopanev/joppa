-- Uploaded JUnit reports and their per-test results. Rows are only appended:
-- a criterion status is computed from them at read time.
CREATE TABLE IF NOT EXISTS joppa.test_reports (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    commit_sha text NOT NULL CHECK (commit_sha ~ '^[0-9a-f]{7,64}$'),
    commit_time timestamptz,
    commit_order bigint NOT NULL,
    branch text NOT NULL,
    default_branch boolean NOT NULL,
    dirty boolean NOT NULL,
    full_run boolean NOT NULL,
    uploader text NOT NULL,
    stamp_hash text NOT NULL,
    junit text NOT NULL,
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (workspace, commit_sha, stamp_hash)
);
CREATE INDEX IF NOT EXISTS test_reports_counting
    ON joppa.test_reports (workspace, commit_order DESC) WHERE default_branch AND NOT dirty;

CREATE TABLE IF NOT EXISTS joppa.test_results (
    report_id bigint NOT NULL REFERENCES joppa.test_reports(id),
    address text NOT NULL,
    status text NOT NULL CHECK (status IN ('pass', 'fail', 'skipped', 'todo')),
    duration_ms bigint,
    message text
);
CREATE INDEX IF NOT EXISTS test_results_by_address ON joppa.test_results (address, report_id);

DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname = 'test_reports_worm' AND tgrelid = 'joppa.test_reports'::regclass) THEN
        CREATE TRIGGER test_reports_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.test_reports FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname = 'test_results_worm' AND tgrelid = 'joppa.test_results'::regclass) THEN
        CREATE TRIGGER test_results_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.test_results FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
END $$;
