-- The parent_hash replay rule: from its epoch on, a save, move or restore of a node stores the
-- contract hash of its parent in the new revision. A journal that exists now gets the epoch
-- after its last event. A workspace that this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'parent_hash', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
-- The current contract hash of the parent of each object, from the objects projection.
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS parent_hash_current text;
