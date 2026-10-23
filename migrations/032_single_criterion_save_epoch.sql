-- The single_criterion_save replay rule: from its epoch on, a save of one criterion gives
-- a new revision only to that criterion and its specification. An older save keeps the
-- revisions it gave to sibling criteria in a replay. A journal that exists now gets the
-- epoch after its last event. A workspace that this server creates stores the epoch at
-- creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'single_criterion_save', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
