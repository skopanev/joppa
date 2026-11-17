-- Each pending search object is one row with the journal position of the write that reached
-- it. A write only inserts rows and a build removes only the rows up to the position of its
-- state, so a write and a build never lock the same row.
ALTER TABLE joppa.search_pending ADD COLUMN IF NOT EXISTS position bigint NOT NULL DEFAULT 0;
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
        WHERE table_schema = 'joppa' AND table_name = 'search_pending' AND column_name = 'id') THEN
        ALTER TABLE joppa.search_pending DROP CONSTRAINT IF EXISTS search_pending_pkey;
        ALTER TABLE joppa.search_pending ADD COLUMN id bigserial PRIMARY KEY;
    END IF;
END $$;
CREATE INDEX IF NOT EXISTS search_pending_by_position ON joppa.search_pending(workspace, position);
CREATE INDEX IF NOT EXISTS search_pending_by_address ON joppa.search_pending(workspace, address);
