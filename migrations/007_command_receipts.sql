-- Original input and resolved results make generated IDs safe to retry.
CREATE TABLE IF NOT EXISTS joppa.command_receipts (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    request_id text NOT NULL,
    actor text NOT NULL,
    input jsonb NOT NULL,
    result jsonb NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (workspace, request_id)
);
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname='command_receipts_worm' AND tgrelid='joppa.command_receipts'::regclass) THEN
        CREATE TRIGGER command_receipts_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.command_receipts FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
END $$;
REVOKE ALL ON joppa.command_receipts FROM PUBLIC;
