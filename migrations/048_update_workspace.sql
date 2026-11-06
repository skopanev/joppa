-- update_workspace replaces rename_workspace. A new reference_prefix of either one keeps the
-- "-<type>-<code>" end of each ID under the new prefix, and the administrative register_item
-- reads it. Old journals keep rename_workspace.
CREATE OR REPLACE FUNCTION joppa.register_event_item() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE ac jsonb; item record; prefix text; alias_id text;
BEGIN
    IF NEW.command->>'op' NOT IN ('domain','capability','consumer','revise','rename_workspace','update_workspace') THEN
        RETURN NEW;
    END IF;
    prefix := NULLIF(current_setting('joppa.reference_prefix',true),'');
    IF prefix IS NULL AND NEW.command->>'op' NOT IN ('rename_workspace','update_workspace') THEN
        SELECT command->>'reference_prefix' INTO prefix FROM joppa.events
            WHERE workspace=NEW.workspace AND command->>'op' IN ('rename_workspace','update_workspace')
                AND command->>'reference_prefix' IS NOT NULL ORDER BY position DESC LIMIT 1;
    END IF;
    prefix := COALESCE(prefix,NEW.workspace);
    IF NEW.command->>'op' IN ('domain','capability','consumer') THEN
        PERFORM joppa.register_item(NEW.workspace,NEW.command->>'op',NEW.command->>'id',prefix);
    ELSIF NEW.command->>'op'='revise' THEN
        PERFORM joppa.register_item(NEW.workspace,'requirement',NEW.command->>'req',prefix);
        FOR ac IN SELECT value FROM jsonb_array_elements(NEW.command->'acs') LOOP
            PERFORM joppa.register_item(NEW.workspace,'ac',ac->>'id',prefix);
        END LOOP;
    END IF;
    IF NEW.command->>'op' IN ('rename_workspace','update_workspace') AND NEW.command->>'reference_prefix' IS NOT NULL THEN
        prefix := NEW.command->>'reference_prefix';
        FOR item IN SELECT DISTINCT ON (kind,local_id) kind,local_id,id FROM joppa.item_ids
            WHERE workspace=NEW.workspace ORDER BY kind,local_id,id
        LOOP
            IF NOT EXISTS (SELECT FROM joppa.item_ids WHERE workspace=NEW.workspace AND kind=item.kind AND local_id=item.local_id AND namespace=prefix) THEN
                alias_id := prefix || substring(item.id from '-[A-Z]-[A-Z0-9]+$');
                INSERT INTO joppa.item_ids(id,workspace,kind,local_id) VALUES(alias_id,NEW.workspace,item.kind,item.local_id)
                    ON CONFLICT DO NOTHING;
                PERFORM joppa.register_item(NEW.workspace,item.kind,item.local_id,prefix);
            END IF;
        END LOOP;
    END IF;
    RETURN NEW;
END $$;

-- The administrative form without a prefix reads the newest prefix of either command.
CREATE OR REPLACE FUNCTION joppa.register_item(w text, k text, local_key text)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE prefix text;
BEGIN
    SELECT command->>'reference_prefix' INTO prefix FROM joppa.events WHERE workspace=w
        AND command->>'op' IN ('rename_workspace','update_workspace') AND command->>'reference_prefix' IS NOT NULL ORDER BY position DESC LIMIT 1;
    PERFORM joppa.register_item(w,k,local_key,coalesce(prefix,w));
END $$;
