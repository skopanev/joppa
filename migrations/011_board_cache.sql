CREATE TABLE IF NOT EXISTS joppa.board_cache (
    workspace text PRIMARY KEY REFERENCES joppa.workspaces(id) ON DELETE CASCADE,
    position bigint NOT NULL,
    state jsonb NOT NULL,
    summary jsonb NOT NULL,
    signals jsonb NOT NULL,
    launch jsonb NOT NULL,
    first_view jsonb NOT NULL
);
ALTER TABLE joppa.board_cache ADD COLUMN IF NOT EXISTS first_view jsonb NOT NULL DEFAULT '{}'::jsonb;
REVOKE UPDATE, DELETE, TRUNCATE ON joppa.board_cache FROM PUBLIC;
CREATE INDEX IF NOT EXISTS events_workspace_req_position
    ON joppa.events (workspace, (command->>'req'), position);
