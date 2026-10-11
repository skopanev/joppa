ALTER TABLE joppa.task_queue ADD COLUMN IF NOT EXISTS eligible boolean NOT NULL DEFAULT false;
ALTER TABLE joppa.task_queue ADD COLUMN IF NOT EXISTS claim_deadline_ms bigint;
CREATE INDEX IF NOT EXISTS task_queue_expired ON joppa.task_queue(workspace,actor,claim_deadline_ms,created_at,sort_req,sort_id)
    WHERE status='in_progress' AND claim_deadline_ms IS NOT NULL;
