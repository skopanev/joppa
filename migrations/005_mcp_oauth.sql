-- Remote MCP OAuth credentials. Only hashes of bearer secrets are persisted.
CREATE TABLE IF NOT EXISTS joppa.oauth_clients (
 client_id text PRIMARY KEY, name text NOT NULL, redirect_uris text[] NOT NULL,
 created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE IF NOT EXISTS joppa.oauth_codes (
 hash text PRIMARY KEY, actor text NOT NULL, client_id text NOT NULL REFERENCES joppa.oauth_clients,
 redirect_uri text NOT NULL, challenge text NOT NULL, resource text NOT NULL,
 expires_at timestamptz NOT NULL DEFAULT clock_timestamp() + interval '2 minutes'
);
CREATE TABLE IF NOT EXISTS joppa.oauth_tokens (
 hash text PRIMARY KEY, kind text NOT NULL CHECK(kind IN ('access','refresh')),
 actor text NOT NULL, client_id text NOT NULL REFERENCES joppa.oauth_clients,
 resource text NOT NULL, family text NOT NULL, expires_at timestamptz NOT NULL
);
CREATE INDEX IF NOT EXISTS oauth_tokens_family_idx ON joppa.oauth_tokens(family);
CREATE TABLE IF NOT EXISTS joppa.oauth_consumed (
 hash text PRIMARY KEY, consumed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE IF NOT EXISTS joppa.oauth_revoked (
 family text PRIMARY KEY, revoked_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
DO $$ DECLARE name text; BEGIN
 FOREACH name IN ARRAY ARRAY['oauth_clients','oauth_codes','oauth_tokens','oauth_consumed','oauth_revoked'] LOOP
  IF NOT EXISTS (SELECT FROM pg_trigger WHERE tgname=name||'_worm' AND tgrelid=('joppa.'||name)::regclass) THEN
   EXECUTE format('CREATE TRIGGER %I BEFORE UPDATE OR DELETE OR TRUNCATE ON joppa.%I FOR EACH STATEMENT EXECUTE FUNCTION joppa.immutable()',name||'_worm',name);
  END IF;
 END LOOP;
END $$;
REVOKE ALL ON joppa.oauth_clients,joppa.oauth_codes,joppa.oauth_tokens,joppa.oauth_consumed,joppa.oauth_revoked FROM PUBLIC;
