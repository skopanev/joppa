-- The any_change_reset replay rule: from its epoch on, any new parent, link or data value of a
-- node resets the criteria at and below it. An older revision keeps its narrower reset in a
-- replay. A journal that exists now gets the epoch after its last event. A workspace that this
-- server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'any_change_reset', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
