-- The own_requirement_revisions replay rule: from its epoch on, a requirement node gets a new
-- revision only when its own data, parent, relations or presence changes. Creating, editing,
-- moving or archiving a child criterion adds no forced revision to any requirement. A journal
-- that exists now gets the epoch after its last event. A workspace that this server creates
-- stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'own_requirement_revisions', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
