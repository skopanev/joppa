-- Every node takes the same fields and operations (src/generic_nodes.rs). No Task queue, no
-- Task review and no Task holder remain: the assignee of a node is data.assignee.
DROP TABLE IF EXISTS joppa.task_queue;
ALTER TABLE joppa.objects DROP COLUMN IF EXISTS review_required;
DROP INDEX IF EXISTS joppa.objects_process_req;
DROP INDEX IF EXISTS joppa.objects_assignee;
CREATE INDEX IF NOT EXISTS objects_data_assignee ON joppa.objects(workspace,(document->'data'->>'assignee')) WHERE present;
