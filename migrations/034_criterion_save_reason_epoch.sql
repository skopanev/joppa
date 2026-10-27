-- The criterion_save_reason replay rule: from its epoch on, the specification revision
-- that a criterion save gives has the reason "Criterion <address> changed: <reason>".
-- An older save keeps the copied reason in a replay. A journal that exists now gets the
-- epoch after its last event. A workspace that this server creates stores the epoch at
-- creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'criterion_save_reason', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
