-- The data pass of migration 062: it moves the stored links of the current rows to relations
-- without a replay, with the functions of 062_relations.sql. The journal, the object revisions
-- and the schema revisions stay as stored: a read maps an older revision on the fly. A journal
-- that exists now gets the epoch of relations_reset after its last event. It runs in `joppa
-- migrate`, and at serve start only when a workspace has no epoch or a state row has no
-- relations, for example after a rollback. It needs no DDL, so the application role can run
-- it. A second run writes nothing.
-- The older release can serve these rows after a rollback (src/relations_stored.rs):
-- - each state_entries object row (value and compact) holds relations and the shadow links;
-- - joppa.objects.document keeps the older form with the shadow links, because the older
--   release compares it with its own form of the revision. Every node of an older journal has
--   that form. Only a node with a flow, a key title or a condition also stores relations.

-- Most state rows have no links: they take empty relations, and their shadow stays [].
UPDATE joppa.state_entries
SET value = jsonb_set(value, '{revisions,0,relations}', '{}'::jsonb),
    compact = CASE WHEN compact IS NULL THEN NULL
        ELSE jsonb_set(compact, '{revisions,0,relations}', '{}'::jsonb) END
WHERE section = 'objects' AND jsonb_array_length(value->'revisions') = 1
    AND value->'revisions'->0->'links' = '[]'::jsonb AND NOT value->'revisions'->0 ? 'relations';

-- A row keeps one revision, and its compact form is the same revision without the body: it
-- takes the same relations and shadow. MATERIALIZED computes the new value once.
WITH n AS MATERIALIZED (
    SELECT workspace, key,
        jsonb_set(value, '{revisions,0}', joppa.revision_relations(value->'revisions'->0)) AS value
    FROM joppa.state_entries
    WHERE section = 'objects' AND jsonb_array_length(value->'revisions') = 1
        AND value->'revisions'->0 ? 'links' AND NOT value->'revisions'->0 ? 'relations'
)
UPDATE joppa.state_entries s
SET value = n.value,
    compact = CASE WHEN s.compact IS NULL THEN NULL
        ELSE jsonb_set(jsonb_set(s.compact, '{revisions,0,relations}', n.value->'revisions'->0->'relations'),
            '{revisions,0,links}', n.value->'revisions'->0->'links') END
FROM n
WHERE s.workspace = n.workspace AND s.section = 'objects' AND s.key = n.key;

-- Any other state row with links, such as a row with more revisions, maps each revision.
UPDATE joppa.state_entries s
SET value = joppa.object_relations(s.value),
    compact = CASE WHEN s.compact IS NULL THEN NULL ELSE joppa.object_relations(s.compact) END
WHERE s.section = 'objects'
    AND jsonb_path_exists(s.value, '$.revisions[*] ? (exists(@.links) && !exists(@.relations))');

-- A document whose links differ from the shadow of their relations, for example with a
-- non-blocking depends_on, gets the shadow. Its object_links rows follow the relations first:
-- the reference rows stay, and a merged edge gives one row.
DELETE FROM joppa.object_links l USING joppa.objects o
WHERE NOT o.document ? 'relations' AND jsonb_array_length(o.document->'links') > 0
    AND o.document->'links' IS DISTINCT FROM joppa.links_shadow(joppa.relations_of_links(o.document->'links'))
    AND l.workspace = o.workspace AND l.source = o.address AND l.relation NOT LIKE 'reference:%';

INSERT INTO joppa.object_links(workspace, source, target, relation, revision, when_label)
SELECT o.workspace, o.address, e.edge->>'to', r.key, (e.edge->>'revision')::integer,
    e.edge->>'when'
FROM joppa.objects o
CROSS JOIN LATERAL jsonb_each(joppa.relations_of_links(o.document->'links')) r(key, value)
CROSS JOIN LATERAL jsonb_array_elements(r.value->'edges') e(edge)
WHERE NOT o.document ? 'relations' AND jsonb_array_length(o.document->'links') > 0
    AND o.document->'links' IS DISTINCT FROM joppa.links_shadow(joppa.relations_of_links(o.document->'links'));

UPDATE joppa.objects o
SET document = o.document || jsonb_build_object('links', joppa.links_shadow(joppa.relations_of_links(o.document->'links')))
WHERE NOT o.document ? 'relations' AND jsonb_array_length(o.document->'links') > 0
    AND o.document->'links' IS DISTINCT FROM joppa.links_shadow(joppa.relations_of_links(o.document->'links'));

INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'relations_reset', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
