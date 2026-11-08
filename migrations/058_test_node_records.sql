-- The rename_keeps_results replay rule: from its epoch on, a new title of a test node resets
-- nothing, and the node keeps its earlier title for the results that an older record stored by
-- title. An older rename resets the node in a replay. A journal that exists now gets the epoch
-- after its last event. A workspace that this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'rename_keeps_results', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
-- A record finds each test node by its title with one index probe for each name.
CREATE INDEX IF NOT EXISTS objects_title ON joppa.objects(workspace,title) WHERE present;
-- A record finds each test node by an earlier title with one index probe for each name.
CREATE INDEX IF NOT EXISTS state_former_test_titles ON joppa.state_entries USING gin (value jsonb_path_ops) WHERE section = 'former_test_titles';
