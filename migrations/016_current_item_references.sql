-- Current writers supply the prefix inside the event insertion transaction.
-- Older binaries and direct administrative imports retain journal compatibility.
CREATE OR REPLACE FUNCTION joppa.register_event_item() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE ac jsonb; item record; prefix text; alias_id text;
BEGIN
    IF NEW.command->>'op' NOT IN ('domain','capability','consumer','revise','rename_workspace') THEN
        RETURN NEW;
    END IF;
    prefix := NULLIF(current_setting('joppa.reference_prefix',true),'');
    IF prefix IS NULL AND NEW.command->>'op'<>'rename_workspace' THEN
        SELECT command->>'reference_prefix' INTO prefix FROM joppa.events
            WHERE workspace=NEW.workspace AND command->>'op'='rename_workspace'
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
    IF NEW.command->>'op'='rename_workspace' AND NEW.command->>'reference_prefix' IS NOT NULL THEN
        prefix := NEW.command->>'reference_prefix';
        FOR item IN SELECT DISTINCT ON (kind,local_id) kind,local_id,id FROM joppa.item_ids
            WHERE workspace=NEW.workspace ORDER BY kind,local_id,id
        LOOP
            IF NOT EXISTS (SELECT FROM joppa.item_ids WHERE workspace=NEW.workspace AND kind=item.kind AND local_id=item.local_id AND namespace=prefix) THEN
                alias_id := prefix || right(item.id,13);
                INSERT INTO joppa.item_ids(id,workspace,kind,local_id) VALUES(alias_id,NEW.workspace,item.kind,item.local_id)
                    ON CONFLICT DO NOTHING;
                PERFORM joppa.register_item(NEW.workspace,item.kind,item.local_id,prefix);
            END IF;
        END LOOP;
    END IF;
    RETURN NEW;
END $$;
