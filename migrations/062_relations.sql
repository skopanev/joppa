-- The relations of a node replace its links. This file holds the DDL of migration 062: the
-- when column of object_links and the functions of the data pass (062_relations_data.sql).
-- Only `joppa migrate` runs it, as the owner of the schema. The mapping is the replay rule of
-- src/relations.rs: a blocking depends_on link is a depends_on edge, every other link is a
-- related edge. The edges keep the order of the links, and the first link to a target in a key
-- gives its edge and its pin. An edge without a pin stores no revision field.
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

-- The rollback shadow of relations, as src/relations_stored.rs writes it: the depends_on edges
-- as blocking links, then the related edges, each with its pin. The older release reads it.
-- Deploy 19b removes the shadow.
CREATE OR REPLACE FUNCTION joppa.links_shadow(relations jsonb)
RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT COALESCE(jsonb_agg(x.link ORDER BY x.k, x.i), '[]'::jsonb) FROM (
        SELECT 0 AS k, t.i, jsonb_build_object('target', t.e->>'to', 'relation', 'depends_on', 'blocking', true)
            || CASE WHEN t.e ? 'revision' THEN jsonb_build_object('revision', t.e->'revision') ELSE '{}'::jsonb END AS link
        FROM jsonb_array_elements(relations->'depends_on'->'edges') WITH ORDINALITY t(e, i)
        UNION ALL
        SELECT 1, t.i, jsonb_build_object('target', t.e->>'to', 'relation', 'related',
            'revision', COALESCE(t.e->'revision', 'null'::jsonb), 'blocking', false)
        FROM jsonb_array_elements(relations->'related'->'edges') WITH ORDINALITY t(e, i)
    ) x
$$;

-- One revision document with links: its relations, and the links become their shadow.
CREATE OR REPLACE FUNCTION joppa.revision_relations_of_links(r jsonb)
RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT r || jsonb_build_object('relations', s.rel, 'links', joppa.links_shadow(s.rel))
    FROM (SELECT joppa.relations_of_links(r->'links') AS rel) s
$$;

-- One revision document: its links become relations. A revision without links needs no
-- aggregate, and most revisions have none. The body is one expression, so a query inlines it.
CREATE OR REPLACE FUNCTION joppa.revision_relations(r jsonb)
RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE
        WHEN NOT r ? 'links' OR r ? 'relations' THEN r
        WHEN jsonb_typeof(r->'links') = 'array' AND jsonb_array_length(r->'links') = 0
            THEN r || '{"relations":{}}'::jsonb
        ELSE joppa.revision_relations_of_links(r) END
$$;

-- One object row of state_entries: each of its revisions.
CREATE OR REPLACE FUNCTION joppa.object_relations(o jsonb)
RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT jsonb_set(o, '{revisions}', COALESCE((SELECT jsonb_agg(joppa.revision_relations(r) ORDER BY i)
        FROM jsonb_array_elements(o->'revisions') WITH ORDINALITY t(r, i)), '[]'::jsonb))
$$;
