-- Permanent public references, independent of agent-authored short names.
CREATE TABLE IF NOT EXISTS joppa.item_ids (
    id text PRIMARY KEY,
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    kind text NOT NULL CHECK (kind IN ('domain','capability','requirement','ac')),
    local_id text NOT NULL,
    UNIQUE (workspace,kind,local_id)
);
CREATE OR REPLACE FUNCTION joppa.register_item(w text, k text, local_key text)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE ref text; alphabet text := 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'; bytes bytea;
BEGIN
    IF EXISTS (SELECT FROM joppa.item_ids WHERE workspace=w AND kind=k AND local_id=local_key) THEN RETURN; END IF;
    LOOP
        ref := w || '-' || CASE k WHEN 'domain' THEN 'D' WHEN 'capability' THEN 'C' WHEN 'requirement' THEN 'R' WHEN 'ac' THEN 'A' END || '-';
        bytes := uuid_send(gen_random_uuid());
        FOR i IN 0..9 LOOP
            ref := ref || substr(alphabet,1 + get_byte(bytes,i) % 36,1);
        END LOOP;
        INSERT INTO joppa.item_ids(id,workspace,kind,local_id) VALUES(ref,w,k,local_key)
            ON CONFLICT DO NOTHING;
        EXIT WHEN EXISTS (SELECT FROM joppa.item_ids WHERE workspace=w AND kind=k AND local_id=local_key);
    END LOOP;
END $$;
CREATE OR REPLACE FUNCTION joppa.register_event_item() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE ac jsonb;
BEGIN
    IF NEW.command->>'op' IN ('domain','capability') THEN
        PERFORM joppa.register_item(NEW.workspace,NEW.command->>'op',NEW.command->>'id');
    ELSIF NEW.command->>'op'='revise' THEN
        PERFORM joppa.register_item(NEW.workspace,'requirement',NEW.command->>'req');
        FOR ac IN SELECT value FROM jsonb_array_elements(NEW.command->'acs') LOOP
            PERFORM joppa.register_item(NEW.workspace,'ac',ac->>'id');
        END LOOP;
    END IF;
    RETURN NEW;
END $$;
DO $$ DECLARE item record; BEGIN
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname='item_ids_worm' AND tgrelid='joppa.item_ids'::regclass) THEN
        CREATE TRIGGER item_ids_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.item_ids FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname='event_item_id' AND tgrelid='joppa.events'::regclass) THEN
        CREATE TRIGGER event_item_id AFTER INSERT ON joppa.events
        FOR EACH ROW EXECUTE FUNCTION joppa.register_event_item();
    END IF;
    FOR item IN SELECT DISTINCT workspace,
        CASE command->>'op' WHEN 'revise' THEN 'requirement' ELSE command->>'op' END AS kind,
        CASE command->>'op' WHEN 'revise' THEN command->>'req' ELSE command->>'id' END AS local_id
        FROM joppa.events WHERE command->>'op' IN ('domain','capability','revise')
        UNION SELECT DISTINCT workspace,'ac',ac->>'id' FROM joppa.events,
            LATERAL jsonb_array_elements(command->'acs') ac WHERE command->>'op'='revise'
    LOOP
        PERFORM joppa.register_item(item.workspace,item.kind,item.local_id);
    END LOOP;
END $$;
REVOKE ALL ON joppa.item_ids FROM PUBLIC;
