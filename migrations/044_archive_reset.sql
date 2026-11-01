-- The archive_own_reset replay rule: from its epoch on, an archive resets the criteria at
-- and below the archived object. An older archive keeps its reset at and below the parent
-- in a replay. A journal that exists now gets the epoch after its last event. A workspace
-- that this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'archive_own_reset', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
