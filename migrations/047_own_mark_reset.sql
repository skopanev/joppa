-- The own_mark_reset replay rule: from its epoch on, a change removes the readiness mark of
-- the changed node only. An older change keeps its removal of each mark at and below the
-- node in a replay. A journal that exists now gets the epoch after its last event. A
-- workspace that this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'own_mark_reset', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
