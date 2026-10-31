-- A new public item ID ends with 8 characters, as a new local object ID has 8 characters.
-- Existing IDs keep their 10 characters: item_ids is append-only. The namespace is the ID
-- without its "-<type>-<code>" end, for codes of each length; for each stored ID it is the
-- same value as the older expression, which assumed 10 characters.
DO $$ BEGIN
    IF pg_get_expr((SELECT d.adbin FROM pg_attrdef d JOIN pg_attribute a ON a.attrelid=d.adrelid AND a.attnum=d.adnum
            WHERE d.adrelid='joppa.item_ids'::regclass AND a.attname='namespace'), 'joppa.item_ids'::regclass) NOT LIKE '%regexp_replace%' THEN
        ALTER TABLE joppa.item_ids DROP COLUMN namespace;
        ALTER TABLE joppa.item_ids ADD COLUMN namespace text
            GENERATED ALWAYS AS (regexp_replace(id, '-[A-Z]-[A-Z0-9]+$', '')) STORED;
        CREATE UNIQUE INDEX IF NOT EXISTS item_ids_per_namespace ON joppa.item_ids(workspace,kind,local_id,namespace);
    END IF;
END $$;
CREATE OR REPLACE FUNCTION joppa.register_item(w text, k text, local_key text, prefix text)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE ref text; alphabet text := 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'; bytes bytea;
BEGIN
    IF EXISTS (SELECT FROM joppa.item_ids WHERE workspace=w AND kind=k AND local_id=local_key AND namespace=prefix) THEN RETURN; END IF;
    LOOP
        ref := prefix || '-' || CASE k WHEN 'domain' THEN 'D' WHEN 'capability' THEN 'C' WHEN 'consumer' THEN 'U' WHEN 'requirement' THEN 'R' WHEN 'ac' THEN 'A' WHEN 'task' THEN 'T' WHEN 'note' THEN 'N' ELSE 'O' END || '-';
        bytes := uuid_send(gen_random_uuid());
        FOR i IN 0..7 LOOP
            ref := ref || substr(alphabet,1 + get_byte(bytes,i) % 36,1);
        END LOOP;
        INSERT INTO joppa.item_ids(id,workspace,kind,local_id) VALUES(ref,w,k,local_key) ON CONFLICT DO NOTHING;
        EXIT WHEN EXISTS (SELECT FROM joppa.item_ids WHERE workspace=w AND kind=k AND local_id=local_key AND namespace=prefix);
    END LOOP;
END $$;
