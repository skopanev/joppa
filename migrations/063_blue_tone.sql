-- Migration 063: the tone grey is blue. This pass moves the stored tones of the current rows
-- without a replay, by the rules of src/status_colors_alias.rs:
-- - joppa.objects: the tone of signal and rollup, the status of test_status, each dim_tones value;
-- - board_cache: the schema revisions of state.tree_schema and the list first_view.statuses;
-- - state_entries: the schema revisions of the meta row.
-- In a status list a grey entry becomes blue. Its colours become #6fa8dc and #8bbbe8 when they
-- are an older default pair, and other colours stay. In the older status_colors map the key grey
-- becomes blue, and its colour stays.
-- The journal, the object revisions, the schema revisions and the command receipts keep every
-- row: a read of an older schema revision gives blue on the fly. The pass needs no DDL, so the
-- application role runs it at serve start. A second run finds no grey and writes nothing.
UPDATE joppa.objects o
SET signal = CASE WHEN o.signal->>'tone' = 'grey'
        THEN jsonb_set(o.signal, '{tone}', '"blue"') ELSE o.signal END,
    rollup = CASE WHEN o.rollup->'signal'->>'tone' = 'grey'
        THEN jsonb_set(o.rollup, '{signal,tone}', '"blue"') ELSE o.rollup END,
    test_status = CASE WHEN o.test_status->>'status' = 'grey'
        THEN jsonb_set(o.test_status, '{status}', '"blue"') ELSE o.test_status END,
    dim_tones = CASE WHEN jsonb_path_exists(o.dim_tones, '$.* ? (@ == "grey")')
        THEN (SELECT jsonb_object_agg(d.key, CASE WHEN d.value = '"grey"' THEN '"blue"'::jsonb
            ELSE d.value END) FROM jsonb_each(o.dim_tones) d)
        ELSE o.dim_tones END
WHERE o.signal->>'tone' = 'grey' OR o.rollup->'signal'->>'tone' = 'grey'
    OR o.test_status->>'status' = 'grey' OR jsonb_path_exists(o.dim_tones, '$.* ? (@ == "grey")');

-- Each distinct schema list with grey, from the meta rows and the board rows.
WITH trees AS MATERIALIZED (
    SELECT DISTINCT t.tree FROM (
        SELECT value->'tree_schema' AS tree FROM joppa.state_entries
        WHERE section = 'meta' AND key = ''
        UNION ALL SELECT state->'tree_schema' FROM joppa.board_cache
    ) t
    WHERE jsonb_typeof(t.tree) = 'array' AND jsonb_path_exists(t.tree,
        '$[*] ? (exists(@.statuses[*] ? (@.tone == "grey")) || exists(@.status_colors.grey))')
), lists AS MATERIALIZED (
    -- Each distinct status list with grey: of a schema revision, or of a board row.
    SELECT DISTINCT l.list FROM (
        SELECT r->'statuses' AS list FROM trees, jsonb_array_elements(trees.tree) r
        UNION ALL SELECT first_view->'statuses' FROM joppa.board_cache
    ) l
    WHERE jsonb_typeof(l.list) = 'array' AND jsonb_path_exists(l.list, '$[*] ? (@.tone == "grey")')
), moved_lists AS (
    SELECT l.list, (SELECT jsonb_agg(CASE WHEN e->>'tone' = 'grey'
            THEN e || '{"tone": "blue"}'::jsonb
                || CASE WHEN (e->>'light', e->>'dark') IN (('#b8b3a8', '#b4b8bf'), ('#c8c3b8', '#8a8f96'))
                    THEN '{"light": "#6fa8dc", "dark": "#8bbbe8"}'::jsonb ELSE '{}'::jsonb END
            ELSE e END ORDER BY i) FROM jsonb_array_elements(l.list) WITH ORDINALITY x(e, i)) AS new
    FROM lists l
), moved_trees AS (
    -- A key blue next to grey takes the colour of grey, as a read of the map does.
    SELECT t.tree, (SELECT jsonb_agg(r
            || CASE WHEN m.new IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('statuses', m.new) END
            || CASE WHEN r->'status_colors' ? 'grey' THEN jsonb_build_object('status_colors',
                ((r->'status_colors') - 'grey') || jsonb_build_object('blue', r->'status_colors'->'grey'))
                ELSE '{}'::jsonb END
            ORDER BY n)
        FROM jsonb_array_elements(t.tree) WITH ORDINALITY x(r, n)
        LEFT JOIN moved_lists m ON m.list = r->'statuses') AS new
    FROM trees t
), meta_rows AS (
    UPDATE joppa.state_entries s
    SET value = jsonb_set(s.value, '{tree_schema}', t.new)
    FROM moved_trees t
    WHERE s.section = 'meta' AND s.key = '' AND s.value->'tree_schema' = t.tree
)
UPDATE joppa.board_cache b
SET state = COALESCE(jsonb_set(b.state, '{tree_schema}', t.new), b.state),
    first_view = COALESCE(jsonb_set(b.first_view, '{statuses}', l.new), b.first_view)
FROM joppa.board_cache old
LEFT JOIN moved_trees t ON t.tree = old.state->'tree_schema'
LEFT JOIN moved_lists l ON l.list = old.first_view->'statuses'
WHERE old.workspace = b.workspace AND (t.tree IS NOT NULL OR l.list IS NOT NULL);
