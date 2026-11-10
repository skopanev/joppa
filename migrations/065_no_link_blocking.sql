-- A depends_on row of object_links blocks, and no other row blocks, so the blocking column
-- repeats the relation. This file drops it. Only `joppa migrate` runs it, as the owner of the
-- schema, before the data passes. joppa.remote_links keeps its own blocking.
ALTER TABLE joppa.object_links DROP COLUMN IF EXISTS blocking;
