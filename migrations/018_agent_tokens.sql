-- Agent tokens: each agent acts under its own identity, never as a person.
-- The workspace owner creates a token for an agent and revokes it; both are appended
-- rows, and the raw token is never stored.
CREATE TABLE IF NOT EXISTS joppa.agent_tokens (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    token_hash text NOT NULL UNIQUE CHECK (length(token_hash) = 64),
    agent text NOT NULL,
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    created_by text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE IF NOT EXISTS joppa.agent_token_revocations (
    token_id bigint PRIMARY KEY REFERENCES joppa.agent_tokens(id),
    revoked_by text NOT NULL,
    revoked_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname = 'agent_tokens_worm' AND tgrelid = 'joppa.agent_tokens'::regclass) THEN
        CREATE TRIGGER agent_tokens_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.agent_tokens FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname = 'agent_token_revocations_worm' AND tgrelid = 'joppa.agent_token_revocations'::regclass) THEN
        CREATE TRIGGER agent_token_revocations_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.agent_token_revocations FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
END $$;
REVOKE ALL ON joppa.agent_tokens, joppa.agent_token_revocations FROM PUBLIC;
