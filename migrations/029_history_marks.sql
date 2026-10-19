-- A History mark: one processed record for one journal position. Rows are only
-- appended, and a mark adds no journal event.
CREATE TABLE IF NOT EXISTS joppa.history_marks (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    position bigint NOT NULL,
    actor text NOT NULL,
    marked_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    reason text NOT NULL CHECK (btrim(reason) <> ''),
    nodes jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(nodes) = 'array' AND jsonb_array_length(nodes) <= 15),
    PRIMARY KEY (workspace, position)
);

DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname = 'history_marks_worm' AND tgrelid = 'joppa.history_marks'::regclass) THEN
        CREATE TRIGGER history_marks_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.history_marks FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
END $$;
