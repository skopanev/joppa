-- Current objects and revision bodies are rebuildable journal projections.
ALTER TABLE joppa.item_ids DROP CONSTRAINT IF EXISTS item_ids_kind_check;
ALTER TABLE joppa.search_documents DROP CONSTRAINT IF EXISTS search_documents_kind_check;
CREATE TABLE IF NOT EXISTS joppa.objects (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    address text NOT NULL,
    kind text NOT NULL,
    local_id text NOT NULL,
    revision integer NOT NULL,
    parent text,
    present boolean NOT NULL,
    title text NOT NULL,
    document jsonb NOT NULL,
    review_required boolean NOT NULL,
    work_ready boolean NOT NULL,
    signal jsonb NOT NULL DEFAULT '{}'::jsonb,
    process jsonb NOT NULL DEFAULT '{}'::jsonb,
    PRIMARY KEY (workspace,address)
);
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS signal jsonb NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS process jsonb NOT NULL DEFAULT '{}'::jsonb;
CREATE INDEX IF NOT EXISTS objects_children ON joppa.objects(workspace,parent,address) WHERE present;
CREATE INDEX IF NOT EXISTS objects_kind ON joppa.objects(workspace,kind,address) WHERE present;
CREATE TABLE IF NOT EXISTS joppa.object_revisions (
    workspace text NOT NULL,
    address text NOT NULL,
    revision integer NOT NULL,
    document jsonb NOT NULL,
    PRIMARY KEY (workspace,address,revision)
);
CREATE TABLE IF NOT EXISTS joppa.object_links (
    workspace text NOT NULL,
    source text NOT NULL,
    target text NOT NULL,
    relation text NOT NULL,
    revision integer,
    blocking boolean NOT NULL,
    PRIMARY KEY (workspace,source,target,relation)
);
CREATE INDEX IF NOT EXISTS object_links_target ON joppa.object_links(workspace,target);
REVOKE ALL ON joppa.objects, joppa.object_revisions, joppa.object_links FROM PUBLIC;

CREATE OR REPLACE FUNCTION joppa.register_item(w text, k text, local_key text, prefix text)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE ref text; alphabet text := 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'; bytes bytea;
BEGIN
    IF EXISTS (SELECT FROM joppa.item_ids WHERE workspace=w AND kind=k AND local_id=local_key AND namespace=prefix) THEN RETURN; END IF;
    LOOP
        ref := prefix || '-' || CASE k WHEN 'domain' THEN 'D' WHEN 'capability' THEN 'C' WHEN 'consumer' THEN 'U' WHEN 'requirement' THEN 'R' WHEN 'ac' THEN 'A' WHEN 'task' THEN 'T' WHEN 'note' THEN 'N' ELSE 'O' END || '-';
        bytes := uuid_send(gen_random_uuid());
        FOR i IN 0..9 LOOP
            ref := ref || substr(alphabet,1 + get_byte(bytes,i) % 36,1);
        END LOOP;
        INSERT INTO joppa.item_ids(id,workspace,kind,local_id) VALUES(ref,w,k,local_key) ON CONFLICT DO NOTHING;
        EXIT WHEN EXISTS (SELECT FROM joppa.item_ids WHERE workspace=w AND kind=k AND local_id=local_key AND namespace=prefix);
    END LOOP;
END $$;


CREATE OR REPLACE FUNCTION joppa.register_item(w text, k text, local_key text)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE prefix text;
BEGIN
    SELECT command->>'reference_prefix' INTO prefix FROM joppa.events WHERE workspace=w
        AND command->>'op'='rename_workspace' AND command->>'reference_prefix' IS NOT NULL ORDER BY position DESC LIMIT 1;
    PERFORM joppa.register_item(w,k,local_key,coalesce(prefix,w));
END $$;
