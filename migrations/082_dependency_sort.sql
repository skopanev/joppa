-- The dependency facts and the creation order of list reads (dependency_sql.rs and
-- dependency_order.rs). The index finds the depends_on edges of one workspace. The column keeps
-- the journal position of the first revision of each object, and the objects projection keeps it.
CREATE INDEX IF NOT EXISTS object_links_relation
    ON joppa.object_links(workspace, relation, source, target);
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS created_position bigint;
UPDATE joppa.objects o
SET created_position = COALESCE(
    (SELECT (r.document->>'position')::bigint
     FROM joppa.object_revisions r
     WHERE r.workspace = o.workspace AND r.address = o.address AND r.revision = 1),
    (o.document->>'position')::bigint,
    0
)
WHERE o.created_position IS NULL;
