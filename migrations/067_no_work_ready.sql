-- Migration 067: the ready filter, work_ready and waits_for are removed. A depends_on edge
-- orders work and refuses a cycle, but it does not make work wait or turn a node yellow.
-- This pass moves the current rows without a replay:
-- - joppa.objects loses the column work_ready.
-- - The stored rollup loses the keys work_ready and waits_for.
-- - A signal with the label "Dependency is not ready" gets the signal that objects::signal now
--   gives: an Inbox input gives its processing state, any other node "Current document".
-- The status projection version 13 makes the catch-up at serve start store the new rollup
-- tones. The journal, the object revisions and the documents keep every row. Only
-- `joppa migrate` runs this file, as the owner of the schema. A second run writes nothing.
UPDATE joppa.objects
SET rollup = rollup - 'work_ready' - 'waits_for'
WHERE rollup ?| ARRAY['work_ready', 'waits_for'];

UPDATE joppa.objects
SET signal = signal || CASE
        WHEN process->>'status' = 'processed'
            THEN '{"tone": "blue", "label": "Input processed"}'::jsonb
        WHEN process->>'status' = 'unprocessed'
            THEN '{"tone": "yellow", "label": "Input awaits processing"}'::jsonb
        ELSE '{"tone": "blue", "label": "Current document"}'::jsonb
    END
WHERE signal->>'label' = 'Dependency is not ready';

ALTER TABLE joppa.objects DROP COLUMN IF EXISTS work_ready;
