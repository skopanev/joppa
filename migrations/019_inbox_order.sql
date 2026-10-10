-- The schema selects Inbox types. This index supports their pending-first order.
CREATE INDEX IF NOT EXISTS objects_inbox_order
    ON joppa.objects(workspace,kind,(inbox_status='processed'),address);
