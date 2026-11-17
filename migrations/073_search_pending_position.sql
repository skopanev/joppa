-- Each pending search object keeps the journal position of the last write that reached it.
-- A builder removes only the objects up to the position of its state, so a write that commits
-- during a build keeps its objects for the next build.
ALTER TABLE joppa.search_pending ADD COLUMN IF NOT EXISTS position bigint NOT NULL DEFAULT 0;
