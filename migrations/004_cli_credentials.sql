-- Browser-approved client credentials. Raw keys are returned once, never stored.
CREATE TABLE IF NOT EXISTS joppa.cli_credentials (
    token_hash text PRIMARY KEY CHECK (length(token_hash) = 64),
    actor text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL DEFAULT clock_timestamp() + interval '90 days'
);
CREATE TABLE IF NOT EXISTS joppa.revoked_cli_credentials (
    token_hash text PRIMARY KEY REFERENCES joppa.cli_credentials(token_hash),
    revoked_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX IF NOT EXISTS cli_credentials_actor_idx ON joppa.cli_credentials(actor);
DO $$ BEGIN
    IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname='cli_credentials_worm' AND tgrelid='joppa.cli_credentials'::regclass) THEN
        CREATE TRIGGER cli_credentials_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.cli_credentials FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
        CREATE TRIGGER revoked_cli_credentials_worm BEFORE UPDATE OR DELETE OR TRUNCATE
        ON joppa.revoked_cli_credentials FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable();
    END IF;
END $$;
REVOKE ALL ON joppa.cli_credentials, joppa.revoked_cli_credentials FROM PUBLIC;
