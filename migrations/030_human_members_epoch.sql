-- The human_members replay rule: from its epoch on, the member command accepts only people.
-- Agents and runners enroll through the internal enroll_service event. An older member
-- event keeps its role in a replay. A journal that exists now gets the epoch after its last
-- event. A workspace that this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'human_members', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
