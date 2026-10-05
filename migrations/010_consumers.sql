-- Consumers are permanent workspace objects with public references.
ALTER TABLE joppa.item_ids DROP CONSTRAINT IF EXISTS item_ids_kind_check;
DO $$ BEGIN
    IF to_regclass('joppa.objects') IS NULL THEN
        ALTER TABLE joppa.item_ids ADD CONSTRAINT item_ids_kind_check
            CHECK (kind IN ('domain','capability','consumer','requirement','ac'));
    END IF;
END $$;

CREATE OR REPLACE FUNCTION joppa.register_item(w text, k text, local_key text)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE ref text; alphabet text := 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'; bytes bytea; prefix text;
BEGIN
    SELECT command->>'reference_prefix' INTO prefix FROM joppa.events WHERE workspace=w
        AND command->>'op'='rename_workspace' AND command->>'reference_prefix' IS NOT NULL ORDER BY position DESC LIMIT 1;
    prefix := coalesce(prefix,w);
    IF EXISTS (SELECT FROM joppa.item_ids WHERE workspace=w AND kind=k AND local_id=local_key AND namespace=prefix) THEN RETURN; END IF;
    LOOP
        ref := prefix || '-' || CASE k WHEN 'domain' THEN 'D' WHEN 'capability' THEN 'C' WHEN 'consumer' THEN 'U' WHEN 'requirement' THEN 'R' WHEN 'ac' THEN 'A' END || '-';
        bytes := uuid_send(gen_random_uuid());
        FOR i IN 0..9 LOOP
            ref := ref || substr(alphabet,1 + get_byte(bytes,i) % 36,1);
        END LOOP;
        INSERT INTO joppa.item_ids(id,workspace,kind,local_id) VALUES(ref,w,k,local_key)
            ON CONFLICT DO NOTHING;
        EXIT WHEN EXISTS (SELECT FROM joppa.item_ids WHERE workspace=w AND kind=k AND local_id=local_key AND namespace=prefix);
    END LOOP;
END $$;

CREATE OR REPLACE FUNCTION joppa.register_event_item() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE ac jsonb; item record; prefix text; alias_id text;
BEGIN
    IF NEW.command->>'op' IN ('domain','capability','consumer') THEN
        PERFORM joppa.register_item(NEW.workspace,NEW.command->>'op',NEW.command->>'id');
    ELSIF NEW.command->>'op'='revise' THEN
        PERFORM joppa.register_item(NEW.workspace,'requirement',NEW.command->>'req');
        FOR ac IN SELECT value FROM jsonb_array_elements(NEW.command->'acs') LOOP
            PERFORM joppa.register_item(NEW.workspace,'ac',ac->>'id');
        END LOOP;
    END IF;
    IF NEW.command->>'op'='rename_workspace' AND NEW.command->>'reference_prefix' IS NOT NULL THEN
        prefix := NEW.command->>'reference_prefix';
        FOR item IN SELECT DISTINCT ON (kind,local_id) kind,local_id,id FROM joppa.item_ids
            WHERE workspace=NEW.workspace ORDER BY kind,local_id,id
        LOOP
            IF NOT EXISTS (SELECT FROM joppa.item_ids WHERE workspace=NEW.workspace AND kind=item.kind AND local_id=item.local_id AND namespace=prefix) THEN
                alias_id := prefix || right(item.id,13);
                INSERT INTO joppa.item_ids(id,workspace,kind,local_id) VALUES(alias_id,NEW.workspace,item.kind,item.local_id)
                    ON CONFLICT DO NOTHING;
                PERFORM joppa.register_item(NEW.workspace,item.kind,item.local_id);
            END IF;
        END LOOP;
    END IF;
    RETURN NEW;
END $$;

DO $$ DECLARE item record; BEGIN
    FOR item IN SELECT DISTINCT workspace,command->>'id' AS local_id FROM joppa.events
        WHERE command->>'op'='consumer'
    LOOP
        PERFORM joppa.register_item(item.workspace,'consumer',item.local_id);
    END LOOP;
END $$;
