-- Names are permanent aliases. Renaming adds a name; journal storage keys never move.
CREATE TABLE IF NOT EXISTS joppa.workspace_aliases (
 name text PRIMARY KEY, workspace text NOT NULL REFERENCES joppa.workspaces(id),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX IF NOT EXISTS workspace_aliases_workspace_idx ON joppa.workspace_aliases(workspace);
INSERT INTO joppa.workspace_aliases(name,workspace) SELECT id,id FROM joppa.workspaces ON CONFLICT DO NOTHING;
DO $$ BEGIN
 IF NOT EXISTS(SELECT FROM pg_trigger WHERE tgname='workspace_aliases_worm' AND tgrelid='joppa.workspace_aliases'::regclass) THEN
  CREATE TRIGGER workspace_aliases_worm BEFORE UPDATE OR DELETE OR TRUNCATE ON joppa.workspace_aliases
  FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
 END IF;
END $$;
REVOKE ALL ON joppa.workspace_aliases FROM PUBLIC;
