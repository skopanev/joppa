-- Speed fixes of SKK 3666 (store_history_filters.rs). The History filters read indexes: the
-- function gives the raw target address of an event before alias resolution, as
-- history_entry::address does. The indexes take no table rewrite, and the events trigger fires
-- only on UPDATE, DELETE and TRUNCATE.
CREATE OR REPLACE FUNCTION joppa.event_raw_address(command jsonb) RETURNS text
LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
SELECT
    CASE
    WHEN command->>'op'='save_object' THEN (command->>'kind')||':'||(command->>'id')
    WHEN command->>'op'='record_report' THEN NULL
    WHEN command->>'op' IN ('assess','claim_planning','finish_planning','release_planning') THEN
    CASE WHEN COALESCE(command->>'ac','')<>'' THEN
    CASE WHEN strpos(command->>'ac',':')>0 THEN command->>'ac' ELSE 'ac:'||(command->>'ac') END
    ELSE NULLIF(command->>'target','') END
    WHEN command->>'op'='comment' THEN
    CASE WHEN COALESCE(command->>'ac','')<>'' THEN
    CASE WHEN strpos(command->>'ac',':')>0 THEN command->>'ac' ELSE 'ac:'||(command->>'ac') END
    WHEN COALESCE(command->>'req','')<>'' THEN
    CASE WHEN strpos(command->>'req',':')>0 THEN command->>'req' ELSE 'requirement:'||(command->>'req') END
    END
    WHEN command->>'op'='revise' AND COALESCE(command->>'req','')<>'' THEN
    CASE WHEN strpos(command->>'req',':')>0 THEN command->>'req' ELSE 'requirement:'||(command->>'req') END
    WHEN command->>'op' IN ('domain','capability','consumer','edit_domain','edit_capability','edit_consumer','set_domain_archived') AND COALESCE(command->>'id','')<>'' THEN
    regexp_replace(regexp_replace(command->>'op','^(edit_|set_)',''),'_archived$','')||':'||(command->>'id')
    WHEN command->>'op' IN ('task','claim_task','claim_next_task','amend_task','release_task','block_task','unblock_task','complete_task','review_task','retire_task','heartbeat_task') AND COALESCE(command->>'id','')<>'' THEN
    CASE WHEN COALESCE(command->>'req','')<>'' THEN 'task:'||(command->>'req')||'/'||(command->>'id')
    WHEN strpos(command->>'id',':')>0 THEN command->>'id'
    ELSE 'task:'||(command->>'id') END
    WHEN COALESCE(command->>'target','')<>'' THEN command->>'target'
    WHEN COALESCE(command->>'req','')<>'' THEN
    CASE WHEN strpos(command->>'req',':')>0 THEN command->>'req' ELSE 'requirement:'||(command->>'req') END
    END
$$;
CREATE INDEX IF NOT EXISTS events_workspace_op_position
    ON joppa.events (workspace, (command ->> 'op'), position);
CREATE INDEX IF NOT EXISTS events_workspace_alias_position
    ON joppa.events (workspace, alias, position) WHERE alias IS NOT NULL;
CREATE INDEX IF NOT EXISTS events_workspace_address_position
    ON joppa.events (workspace, joppa.event_raw_address(command), position)
    WHERE joppa.event_raw_address(command) IS NOT NULL;
CREATE INDEX IF NOT EXISTS events_workspace_kind_position
    ON joppa.events (workspace, split_part(joppa.event_raw_address(command), ':', 1), position)
    WHERE joppa.event_raw_address(command) IS NOT NULL;
