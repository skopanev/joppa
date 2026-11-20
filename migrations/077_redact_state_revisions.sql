-- A redaction or a replay rewrites the masked text of stored state revisions while keeping their primary key.
CREATE OR REPLACE FUNCTION joppa.protect_state_revision()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (current_setting('joppa.redacting', true) IS NOT DISTINCT FROM '1'
        OR current_setting('joppa.replaying', true) IS NOT DISTINCT FROM '1')
        AND (NEW.workspace,NEW.section,NEW.key,NEW.revision) IS NOT DISTINCT FROM (OLD.workspace,OLD.section,OLD.key,OLD.revision) THEN
        RETURN NEW;
    END IF;
    IF (NEW.workspace,NEW.section,NEW.key,NEW.revision) IS DISTINCT FROM (OLD.workspace,OLD.section,OLD.key,OLD.revision)
        OR (NEW.value IS DISTINCT FROM OLD.value AND (OLD.section <> 'requirements'
            OR joppa.verification_contract(NEW.value) IS DISTINCT FROM joppa.verification_contract(OLD.value))) THEN
        RAISE EXCEPTION 'stored revision contract is immutable';
    END IF;
    RETURN NEW;
END $$;
