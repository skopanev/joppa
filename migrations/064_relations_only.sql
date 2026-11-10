-- Migration 064: a current row stores relations and no links. This pass removes the links of
-- the current rows without a replay, after the data pass of 062:
-- - joppa.objects.document becomes the stored form of its revision: its relations and no links;
-- - each revision of a state_entries object row (value and compact) loses its links.
-- A document or a revision with links and no relations takes the relations of its links by the
-- replay rule (joppa.revision_relations of 062). Links [] give relations {}.
-- The journal, the object revisions, the schema revisions and the command receipts keep every
-- row: a read of a revision before 062 maps its links on the fly. The pass needs no DDL, so the
-- application role runs it at serve start. A second run finds no links and writes nothing.
UPDATE joppa.objects
SET document = joppa.revision_relations(document) - 'links'
WHERE document ? 'links';

-- Most state rows keep one revision that has relations: they only lose the links.
UPDATE joppa.state_entries
SET value = value #- '{revisions,0,links}',
    compact = compact #- '{revisions,0,links}'
WHERE section = 'objects' AND jsonb_array_length(value->'revisions') = 1
    AND value->'revisions'->0 ? 'relations' AND value->'revisions'->0 ? 'links'
    AND (compact IS NULL OR compact->'revisions'->0 ? 'relations');

-- Any other state row with links maps each revision.
UPDATE joppa.state_entries
SET value = jsonb_set(value, '{revisions}', COALESCE((SELECT jsonb_agg(joppa.revision_relations(r) - 'links' ORDER BY i)
        FROM jsonb_array_elements(value->'revisions') WITH ORDINALITY t(r, i)), '[]'::jsonb)),
    compact = CASE WHEN compact IS NULL THEN NULL
        ELSE jsonb_set(compact, '{revisions}', COALESCE((SELECT jsonb_agg(joppa.revision_relations(r) - 'links' ORDER BY i)
            FROM jsonb_array_elements(compact->'revisions') WITH ORDINALITY t(r, i)), '[]'::jsonb)) END
WHERE section = 'objects' AND (jsonb_path_exists(value, '$.revisions[*].links')
    OR jsonb_path_exists(compact, '$.revisions[*].links'));
