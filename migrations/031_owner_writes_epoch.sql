-- The owner_writes replay rule: from its epoch on, only the workspace owner sets the tree
-- schema, archives and restores. An older event of an agent member keeps its effect in a
-- replay. A journal that exists now gets the epoch after its last event. A workspace that
-- this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'owner_writes', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
