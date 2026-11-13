-- A workspace has 0 or 1 read key. The key reads only POST /api/data of its workspace.
-- The table keeps only the SHA-256 hex digest of the key, never the key. The foreign key
-- and joppa.purge_workspace remove the row with its workspace.
CREATE TABLE IF NOT EXISTS joppa.workspace_read_keys (
    workspace text PRIMARY KEY REFERENCES joppa.workspaces(id) ON DELETE CASCADE,
    token_sha256 text NOT NULL UNIQUE,
    created_by text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
REVOKE ALL ON joppa.workspace_read_keys FROM PUBLIC;
