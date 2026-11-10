-- The human_member_rights replay rule: from its epoch on, a new domain or capability is
-- confirmed when any human member saves it. An older save keeps the older rule in a replay:
-- only the creator of the workspace confirmed. A journal that exists now gets the epoch after
-- its last event. A workspace that this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'human_member_rights', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
