-- The document overrides and the archived objects of a workspace are one state row for each
-- address. The meta row kept them as lists, and each write serialized and stored the lists.
INSERT INTO joppa.state_entries(workspace,section,key,value)
SELECT s.workspace,x.section,x.key,'true'::jsonb
FROM joppa.state_entries s
CROSS JOIN LATERAL (
    SELECT 'document_overrides' AS section,e AS key FROM jsonb_array_elements_text(CASE WHEN jsonb_typeof(s.value->'document_overrides')='array' THEN s.value->'document_overrides' ELSE '[]' END) e
    UNION ALL
    SELECT 'archived_objects',e FROM jsonb_array_elements_text(CASE WHEN jsonb_typeof(s.value->'archived_objects')='array' THEN s.value->'archived_objects' ELSE '[]' END) e
) x
WHERE s.section='meta'
ON CONFLICT (workspace,section,key) DO NOTHING;
UPDATE joppa.state_entries SET value=value-'document_overrides'-'archived_objects'
WHERE section='meta' AND (value ? 'document_overrides' OR value ? 'archived_objects');
