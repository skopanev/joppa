-- Each new or changed scalar in the data of an object holds at most 262144 characters, the
-- ceiling of the field_max_length of a type. The server checks the limit of each type. A
-- stored value of any length may stay in a later revision unchanged. Journal replays and
-- projection rebuilds set joppa.replaying to '1' and skip the check. A redaction only
-- removes text and also sets it.
CREATE OR REPLACE FUNCTION joppa.new_long_field(new_data jsonb, old_data jsonb, max_len integer)
RETURNS boolean LANGUAGE sql STABLE AS $$
    WITH RECURSIVE leaves(path, value) AS (
        SELECT ARRAY[]::text[], COALESCE(new_data, '{}'::jsonb)
        UNION ALL
        SELECT l.path || c.key, c.value
        FROM leaves l
        CROSS JOIN LATERAL (
            SELECT e.key, e.value
            FROM jsonb_each(CASE WHEN jsonb_typeof(l.value) = 'object' THEN l.value ELSE '{}'::jsonb END) e
            UNION ALL
            SELECT (a.n - 1)::text, a.v
            FROM jsonb_array_elements(CASE WHEN jsonb_typeof(l.value) = 'array' THEN l.value ELSE '[]'::jsonb END)
                WITH ORDINALITY AS a(v, n)
        ) c
    )
    SELECT EXISTS (
        SELECT 1 FROM leaves
        WHERE jsonb_typeof(value) IN ('string', 'number', 'boolean')
          AND char_length(value #>> '{}') > max_len
          AND (old_data IS NULL OR old_data #> path IS DISTINCT FROM value)
    )
$$;

CREATE OR REPLACE FUNCTION joppa.field_length_limit() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    previous jsonb;
BEGIN
    IF current_setting('joppa.replaying', true) IS NOT DISTINCT FROM '1' THEN
        RETURN NEW;
    END IF;
    IF TG_OP = 'UPDATE' THEN
        IF NEW.document->'data' IS NOT DISTINCT FROM OLD.document->'data' THEN
            RETURN NEW;
        END IF;
        previous := OLD.document->'data';
    ELSIF TG_TABLE_NAME = 'object_revisions' THEN
        SELECT r.document->'data' INTO previous FROM joppa.object_revisions r
        WHERE r.workspace = NEW.workspace AND r.address = NEW.address AND r.revision < NEW.revision
        ORDER BY r.revision DESC LIMIT 1;
    ELSE
        SELECT o.document->'data' INTO previous FROM joppa.objects o
        WHERE o.workspace = NEW.workspace AND o.address = NEW.address;
    END IF;
    IF joppa.new_long_field(NEW.document->'data', previous, 262144) THEN
        RAISE EXCEPTION 'an object field is at most 262144 characters' USING ERRCODE = 'check_violation';
    END IF;
    RETURN NEW;
END $$;

DROP TRIGGER IF EXISTS objects_field_length_limit ON joppa.objects;
CREATE TRIGGER objects_field_length_limit BEFORE INSERT OR UPDATE OF document ON joppa.objects
    FOR EACH ROW EXECUTE FUNCTION joppa.field_length_limit();
DROP TRIGGER IF EXISTS object_revisions_field_length_limit ON joppa.object_revisions;
CREATE TRIGGER object_revisions_field_length_limit BEFORE INSERT OR UPDATE OF document ON joppa.object_revisions
    FOR EACH ROW EXECUTE FUNCTION joppa.field_length_limit();
