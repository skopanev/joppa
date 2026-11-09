-- The source_change_reset replay rule: from its epoch on, a new title, body or file of a node of
-- a source type resets each criterion that names it or links with it. An older event resets
-- none of them in a replay. A journal that exists now gets the epoch after its last event. A
-- workspace that this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'source_change_reset', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
