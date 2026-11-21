-- A workspace has 0 or 1 write key. The key calls POST /mcp only for its workspace and only
-- for the writes that key_write in the tree schema allows. The table keeps only the SHA-256
-- hex digest of the key, never the key. joppa.purge_workspace removes the row with its workspace.
CREATE TABLE IF NOT EXISTS joppa.workspace_write_keys (
    workspace text PRIMARY KEY REFERENCES joppa.workspaces(id) ON DELETE CASCADE,
    token_sha256 text NOT NULL UNIQUE,
    created_by text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
REVOKE ALL ON joppa.workspace_write_keys FROM PUBLIC;
