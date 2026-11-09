-- The relations of a node replace its links. This pass moves the stored links of the current
-- rows to relations without a replay, by the replay rule of src/relations.rs: a blocking
-- depends_on link is a depends_on edge, every other link is a related edge. The edges keep the
-- order of the links, and the first link to a target in a key gives its edge and its pin. A
-- revision without a pin stores no revision field, as serde writes an edge.
-- The pass rewrites only the current rows: state_entries section objects (each revision of
-- value, and compact), joppa.objects.document and joppa.object_links. The journal, the object
-- revisions and the schema revisions stay as stored: a read maps an older revision on the fly.
-- A journal that exists now gets the epoch of relations_reset after its last event. A
-- workspace that this server creates stores the epoch at creation. A second run finds no
-- links and writes nothing.
ALTER TABLE joppa.object_links ADD COLUMN IF NOT EXISTS when_label text;

CREATE OR REPLACE FUNCTION joppa.relations_of_links(links jsonb)
RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT COALESCE(jsonb_object_agg(g.key, jsonb_build_object('edges', g.edges)), '{}'::jsonb)
    FROM (
        SELECT f.key, jsonb_agg(jsonb_strip_nulls(jsonb_build_object('to', f.target, 'revision', f.revision)) ORDER BY f.ord) AS edges
        FROM (
            SELECT DISTINCT ON (l.key, l.target) l.key, l.target, l.revision, l.ord
            FROM (
                SELECT x.ord, x.link->>'target' AS target, x.link->'revision' AS revision,
                    CASE WHEN x.link->>'relation' = 'depends_on' AND (x.link->>'blocking')::boolean
                        THEN 'depends_on' ELSE 'related' END AS key
                FROM jsonb_array_elements(COALESCE(links, '[]'::jsonb)) WITH ORDINALITY x(link, ord)
            ) l
            ORDER BY l.key, l.target, l.ord
        ) f
        GROUP BY f.key
    ) g
$$;

-- One revision document: its links become relations. A revision without links needs no
-- aggregate: most revisions have none.
CREATE OR REPLACE FUNCTION joppa.revision_relations(r jsonb)
RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE WHEN r ? 'links' AND NOT r ? 'relations'
        THEN (r - 'links') || jsonb_build_object('relations',
            CASE WHEN jsonb_typeof(r->'links') = 'array' AND jsonb_array_length(r->'links') = 0
                THEN '{}'::jsonb ELSE joppa.relations_of_links(r->'links') END)
        ELSE r END
$$;

-- One object row of state_entries: each of its revisions.
CREATE OR REPLACE FUNCTION joppa.object_relations(o jsonb)
RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT jsonb_set(o, '{revisions}', COALESCE((SELECT jsonb_agg(joppa.revision_relations(r) ORDER BY i)
        FROM jsonb_array_elements(o->'revisions') WITH ORDINALITY t(r, i)), '[]'::jsonb))
$$;

-- A row keeps one revision, and its compact form is the same revision without the body: it
-- takes the same relations. A row with more revisions maps each of them.
UPDATE joppa.state_entries s
SET value = n.value,
    compact = CASE WHEN s.compact IS NULL THEN NULL
        ELSE jsonb_set(s.compact #- '{revisions,0,links}', '{revisions,0,relations}',
            n.value->'revisions'->0->'relations') END
FROM (
    SELECT workspace, key,
        jsonb_set(value, '{revisions,0}', joppa.revision_relations(value->'revisions'->0)) AS value
    FROM joppa.state_entries
    WHERE section = 'objects' AND jsonb_array_length(value->'revisions') = 1
        AND value->'revisions'->0 ? 'links' AND NOT value->'revisions'->0 ? 'relations'
) n
WHERE s.workspace = n.workspace AND s.section = 'objects' AND s.key = n.key;

UPDATE joppa.state_entries s
SET value = joppa.object_relations(s.value),
    compact = CASE WHEN s.compact IS NULL THEN NULL ELSE joppa.object_relations(s.compact) END
WHERE s.section = 'objects' AND jsonb_array_length(s.value->'revisions') <> 1
    AND (jsonb_path_exists(s.value, '$.revisions[*].links')
        OR COALESCE(jsonb_path_exists(s.compact, '$.revisions[*].links'), false));

-- The object_links rows of a moved document follow its relations: the reference rows stay,
-- and a merged edge gives one row.
CREATE TEMP TABLE IF NOT EXISTS relations_moved (workspace text, address text) ON COMMIT DROP;
WITH moved AS (
    UPDATE joppa.objects o SET document = joppa.revision_relations(o.document)
    WHERE o.document ? 'links' AND NOT o.document ? 'relations'
    RETURNING o.workspace, o.address
)
INSERT INTO relations_moved SELECT workspace, address FROM moved;

DELETE FROM joppa.object_links l USING relations_moved m
WHERE l.workspace = m.workspace AND l.source = m.address AND l.relation NOT LIKE 'reference:%';

INSERT INTO joppa.object_links(workspace, source, target, relation, revision, blocking, when_label)
SELECT m.workspace, m.address, e.edge->>'to', r.key, (e.edge->>'revision')::integer,
    r.key = 'depends_on', e.edge->>'when'
FROM relations_moved m
JOIN joppa.objects o ON o.workspace = m.workspace AND o.address = m.address
CROSS JOIN LATERAL jsonb_each(o.document->'relations') r(key, value)
CROSS JOIN LATERAL jsonb_array_elements(r.value->'edges') e(edge);

INSERT INTO joppa.replay_epochs(workspace, rule, first_position)
SELECT w.id, 'relations_reset', (SELECT COALESCE(max(e.position), 0) + 1 FROM joppa.events e WHERE e.workspace = w.id)
FROM joppa.workspaces w
ON CONFLICT (workspace, rule) DO NOTHING;
