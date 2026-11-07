-- The forced_keeps_contract replay rule: from its epoch on, a revision that only a child save
-- forces keeps the contract revision of the document. An older one starts a new contract
-- revision in a replay. A journal that exists now gets the epoch at the first forced revision
-- that keeps a contract, else after its last event. A workspace that this server creates
-- stores the epoch at creation.
INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'forced_keeps_contract', COALESCE(
    (SELECT min((r.document->>'position')::bigint)
     FROM joppa.object_revisions r
     JOIN joppa.events e ON e.workspace = r.workspace AND e.position = (r.document->>'position')::bigint
     WHERE r.workspace = w.id
       AND r.document ? 'contract'
       AND e.command->>'op' = 'save_object'
       AND r.address <> ((e.command->>'kind') || ':' || (e.command->>'id'))),
    (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id))
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
