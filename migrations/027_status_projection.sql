-- The status projection: the stored test status of each object and the stored row
-- decoration of each tree node. Every write rebuilds both in its own transaction, inside
-- store_board::project_current, so a read uses them and computes nothing. The journal and
-- the test reports stay the source of truth. The runtime role may already update
-- joppa.objects and joppa.board_cache, so these columns need no new grant.
-- test_status: the JSON of the criterion test status, as the /tests read returns it.
-- rollup: the row fields that the decoration helpers set (work_ready, waits_for,
-- verification_summary, signal, process.verification, process.readiness_blocker).
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS test_status jsonb;
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS rollup jsonb;
-- Every rebuild stores a value for every object, so a NULL is a lost row. A read refuses a
-- workspace with such a row; this index makes that check cheap.
CREATE INDEX IF NOT EXISTS objects_projection_gaps ON joppa.objects(workspace)
    WHERE test_status IS NULL OR rollup IS NULL;
-- board_cache.first_view.status_projection holds the code version and the journal position
-- of the last rebuild. A read refuses a projection that does not match the board position.
