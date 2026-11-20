-- The contract_only_reset replay rule: from its epoch on, a save resets the criteria only for a
-- change of the contract. A new owner, assignee, value of a typed field that does not reset, or
-- source of equal title, body and live files resets nothing. A journal that exists now gets the
-- epoch after its last event. A workspace that this server creates stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'contract_only_reset', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
