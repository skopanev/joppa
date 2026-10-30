-- The owner deletes a workspace, and every row of it leaves the database: its journal,
-- its projections, its receipts, its grants, its tokens and its test reports. The server
-- removes the files that only it names from Spaces after the commit. Every other UPDATE,
-- DELETE and TRUNCATE of a journal table stays refused.
--
-- Only joppa.purge_workspace sets joppa.purge_workspace. The function runs with the
-- rights of its owner, because the application role has no DELETE on the journal tables.
CREATE OR REPLACE FUNCTION joppa.immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' AND COALESCE(current_setting('joppa.purge_workspace', true), '') <> '' THEN
        RETURN NULL;
    END IF;
    RAISE EXCEPTION 'WORM: append a correction; history cannot be changed';
END $$;

CREATE OR REPLACE FUNCTION joppa.purge_workspace(target text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, joppa AS $$
DECLARE
    table_name text;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(target, 91827102));
    PERFORM set_config('joppa.purge_workspace', target, true);
    DELETE FROM joppa.test_results WHERE report_id IN (SELECT id FROM joppa.test_reports WHERE workspace = target);
    DELETE FROM joppa.test_report_receipts WHERE report_id IN (SELECT id FROM joppa.test_reports WHERE workspace = target);
    DELETE FROM joppa.agent_token_revocations WHERE token_id IN (SELECT id FROM joppa.agent_tokens WHERE workspace = target);
    -- Every table with a workspace column, also a table that a later migration adds.
    FOR table_name IN
        SELECT c.table_name FROM information_schema.columns c
        JOIN information_schema.tables t ON t.table_schema = c.table_schema AND t.table_name = c.table_name
        WHERE c.table_schema = 'joppa' AND c.column_name = 'workspace' AND t.table_type = 'BASE TABLE'
        ORDER BY c.table_name
    LOOP
        EXECUTE format('DELETE FROM joppa.%I WHERE workspace = $1', table_name) USING target;
    END LOOP;
    DELETE FROM joppa.workspaces WHERE id = target;
    PERFORM set_config('joppa.purge_workspace', '', true);
END $$;

REVOKE ALL ON FUNCTION joppa.purge_workspace(text) FROM PUBLIC;

-- A workspace that an owner deleted before this migration leaves now.
SELECT joppa.purge_workspace(d.workspace)
FROM (SELECT DISTINCT workspace FROM joppa.events WHERE command->>'op' = 'delete_workspace' ORDER BY workspace) d;
