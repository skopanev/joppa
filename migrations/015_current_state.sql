CREATE TABLE IF NOT EXISTS joppa.state_entries (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    section text NOT NULL,
    key text NOT NULL,
    value jsonb NOT NULL,
    PRIMARY KEY (workspace,section,key)
);
ALTER TABLE joppa.state_entries ADD COLUMN IF NOT EXISTS kind text;
ALTER TABLE joppa.state_entries ADD COLUMN IF NOT EXISTS compact jsonb;
CREATE INDEX IF NOT EXISTS state_members_actor ON joppa.state_entries(key,workspace) WHERE section='members';
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS owner text;
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS description text;
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS document_at text;
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS inbox_status text NOT NULL DEFAULT 'unprocessed';
CREATE INDEX IF NOT EXISTS objects_inbox ON joppa.objects(workspace,kind,inbox_status,address) WHERE present;
CREATE TABLE IF NOT EXISTS joppa.state_revisions (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    section text NOT NULL,
    key text NOT NULL,
    revision integer NOT NULL,
    value jsonb NOT NULL,
    PRIMARY KEY (workspace,section,key,revision)
);
CREATE TABLE IF NOT EXISTS joppa.ac_owners (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    criterion text NOT NULL,
    requirement text NOT NULL,
    PRIMARY KEY (workspace,criterion)
);
REVOKE ALL ON joppa.state_entries, joppa.state_revisions, joppa.ac_owners FROM PUBLIC;

CREATE OR REPLACE FUNCTION joppa.verification_contract(value jsonb)
RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT jsonb_set(value - ARRAY['confirmed','tasks','runs','issues','acceptance','coverage_change','coverage_review','verification_scope','sources'],
        '{acs}', COALESCE((SELECT jsonb_agg(ac - 'assessment') FROM jsonb_array_elements(value->'acs') ac),'[]'::jsonb));
$$;
CREATE OR REPLACE FUNCTION joppa.protect_state_revision()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.workspace,NEW.section,NEW.key,NEW.revision) IS DISTINCT FROM (OLD.workspace,OLD.section,OLD.key,OLD.revision)
        OR (NEW.value IS DISTINCT FROM OLD.value AND (OLD.section <> 'requirements'
            OR joppa.verification_contract(NEW.value) IS DISTINCT FROM joppa.verification_contract(OLD.value))) THEN
        RAISE EXCEPTION 'stored revision contract is immutable';
    END IF;
    RETURN NEW;
END $$;
DROP TRIGGER IF EXISTS state_revision_contract ON joppa.state_revisions;
CREATE TRIGGER state_revision_contract BEFORE UPDATE ON joppa.state_revisions
    FOR EACH ROW EXECUTE FUNCTION joppa.protect_state_revision();

CREATE TABLE IF NOT EXISTS joppa.task_queue (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    address text NOT NULL,
    actor text NOT NULL,
    created_at bigint NOT NULL,
    sort_req text NOT NULL,
    sort_id text NOT NULL,
    status text NOT NULL,
    assignee text,
    purpose text NOT NULL,
    claimable boolean NOT NULL,
    targets text[] NOT NULL,
    subjects text[] NOT NULL,
    data jsonb NOT NULL,
    PRIMARY KEY(workspace,actor,address)
);
CREATE INDEX IF NOT EXISTS task_queue_order ON joppa.task_queue(workspace,actor,created_at,sort_req,sort_id);
CREATE INDEX IF NOT EXISTS task_queue_available ON joppa.task_queue(workspace,actor,created_at,sort_req,sort_id) WHERE claimable;
CREATE INDEX IF NOT EXISTS task_queue_assignee ON joppa.task_queue(workspace,actor,assignee,status,created_at);
CREATE INDEX IF NOT EXISTS task_queue_subjects ON joppa.task_queue USING gin(subjects);
CREATE INDEX IF NOT EXISTS task_queue_targets ON joppa.task_queue USING gin(targets);
CREATE INDEX IF NOT EXISTS objects_all_children ON joppa.objects(workspace,parent,address);
CREATE INDEX IF NOT EXISTS objects_process_req ON joppa.objects(workspace,(process->>'req'),address);
REVOKE ALL ON joppa.task_queue FROM PUBLIC;
