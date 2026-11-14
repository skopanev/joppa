-- The own_alias replay rule: from its epoch on, a revision that a write copies from the last
-- one, such as an archive or a restore, takes the alias of that write. An older event keeps
-- the copied alias in a replay. A journal that exists now gets the epoch after its last
-- event. A workspace that this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'own_alias', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
