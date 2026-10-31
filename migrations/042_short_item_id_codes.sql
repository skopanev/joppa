-- Every public item ID ends with an 8-character code. An older 10-character code keeps its
-- first 8 characters, and a copied older ID still resolves through joppa.short_item_id.
CREATE OR REPLACE FUNCTION joppa.short_item_id(id text) RETURNS text LANGUAGE sql IMMUTABLE AS $$
    SELECT regexp_replace(id, '-([A-Z])-([A-Z0-9]{8})[A-Z0-9]+$', '-\1-\2')
$$;
DO $$ BEGIN
    IF EXISTS (SELECT FROM joppa.item_ids WHERE id ~ '-[A-Z]-[A-Z0-9]{9,}$') THEN
        ALTER TABLE joppa.item_ids DISABLE TRIGGER item_ids_worm;
        UPDATE joppa.item_ids SET id = joppa.short_item_id(id) WHERE id ~ '-[A-Z]-[A-Z0-9]{9,}$';
        ALTER TABLE joppa.item_ids ENABLE TRIGGER item_ids_worm;
    END IF;
END $$;
