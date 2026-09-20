-- Additive migration: public tables from 0.1 remain untouched.
CREATE SCHEMA IF NOT EXISTS joppa;
-- Verified Google identities are bound once; neither email initials nor browser input select an actor.
CREATE TABLE IF NOT EXISTS joppa.google_identities (
    actor text PRIMARY KEY,
    google_sub text NOT NULL UNIQUE,
    email text NOT NULL UNIQUE,
    hosted_domain text,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
-- Browser credentials survive application restarts. Raw bearer secrets are never stored.
CREATE TABLE IF NOT EXISTS joppa.browser_sessions (
    token_hash text PRIMARY KEY CHECK (length(token_hash) = 64),
    actor text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL
);
CREATE TABLE IF NOT EXISTS joppa.revoked_sessions (
    token_hash text PRIMARY KEY REFERENCES joppa.browser_sessions(token_hash),
    revoked_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE IF NOT EXISTS joppa.workspaces (
    id text PRIMARY KEY CHECK (length(btrim(id)) > 0),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE IF NOT EXISTS joppa.events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    position bigint NOT NULL CHECK (position > 0),
    request_id text NOT NULL,
    actor text NOT NULL CHECK (length(btrim(actor)) > 0),
    recorded_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    command jsonb NOT NULL CHECK (jsonb_typeof(command) = 'object'),
    UNIQUE (workspace, position),
    UNIQUE (workspace, request_id)
);
CREATE OR REPLACE FUNCTION joppa.immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'WORM: append a correction; history cannot be changed';
END $$;
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname = 'events_worm' AND tgrelid = 'joppa.events'::regclass) THEN
        CREATE TRIGGER events_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.events FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname = 'workspaces_worm' AND tgrelid = 'joppa.workspaces'::regclass) THEN
        CREATE TRIGGER workspaces_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.workspaces FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
END $$;
REVOKE ALL ON SCHEMA joppa FROM PUBLIC;
REVOKE ALL ON ALL TABLES IN SCHEMA joppa FROM PUBLIC;
