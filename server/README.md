# Single-server deployment

Target: Ubuntu 24.04 LTS, one shared vCPU, 2 GB RAM. PostgreSQL 16 stores the
journal, pgvector embeddings and full-text search. DeepInfra performs embedding
inference. Joppa has its own Google OAuth client and invited account list.

MCP search indexes current product wording. Search is always hybrid: the server
needs the private DeepInfra key in `search.env` and does not start without it.
See [search setup and limits](../docs/search.md).

## Deploy

From a clean, committed checkout, with root SSH access to an Ubuntu 24.04 host:

```sh
server/deploy.sh root@SERVER_IP app.example.com /private/joppa-config
# Only for the first deployment to a host without a Joppa database:
server/deploy.sh root@SERVER_IP app.example.com /private/joppa-config /private/initial.dump
```

Alternatively, copy the root `.env.example` to the ignored `.env`, fill its
deployment parameters, then load it explicitly and run without arguments:

```sh
set -a; . ./.env; set +a
server/deploy.sh
```

The script does not silently source files. `.env.example` contains placeholders;
`.env` and the private JSON/Spaces files never belong in Git. Clear
`JOPPA_INITIAL_DUMP` after the first deployment.

Set `JOPPA_SSH_CONFIG` to use a private SSH configuration file. The host key must
already be trusted. The domain must point at the host. No hostnames, account
addresses, SSH keys, product data or live credentials belong in Git.

The private config directory must contain `tokens.json` and `login-users.json`
(formats in the root README). It may also contain `google-client.json`,
`access-rules.json`, `backup.env`, `storage.env` and `search.env`. Only those seven files are transferred. Optional
files omitted on an update keep their existing server configuration. Without a
Google client on the server, the authorization page loads but sign-in stays disabled.

`access-rules.json` assigns workspaces by verified Google Workspace domain, with
exact email overrides (see the anonymized example). An email rule replaces the
domain rule; an empty list denies access. Domains match exactly and require
Google's verified `hd` claim. Personal Google accounts need an explicit email rule.
Rules are reread for each protected request: removing access takes effect for
existing sessions, without rewriting journal history. Adding a new workspace
grant enrolls the user on their next sign-in. Unknown workspaces fail enrollment.
Each account has its own identity; API keys and browser sessions use the same
store authorization checks. MCP clients connect directly over HTTPS with OAuth.
Keep database credentials private: database administrators are outside this access boundary.

`login-users.json` can retain explicit actor aliases for existing people. Accounts
admitted through access rules receive a stable identity derived from their Google
subject when no alias exists. Two Google accounts cannot share an actor or claim
each other's identity. Automatic enrollment records a `joppa-access` system event.

The deploy script archives tracked files from HEAD, provisions missing dependencies,
builds with one job, creates the database and a restricted runtime role, applies
additive migrations, installs systemd services and configures Caddy HTTPS. It
generates the database password on the host. An initial dump is accepted only
when the database does not exist; updates never restore over existing data.
Transferred dumps and staging files are removed after use. Interrupted network
connections can leave a private remote staging directory; the script reports
cleanup failures so it can be removed after the interrupted deployment ends.

A failed application health check restores the previous binary when available;
database migrations and configuration changes are not rolled back. The final
check calls the public HTTPS health endpoint. This does not verify Google login
or establish Spaces retention: those require their own acceptance checks.

When backup credentials are present, the daily timer is enabled for 03:30 UTC
with up to 15 minutes of jitter. Run the backup service once and verify a Spaces
download and restore. Local dumps exist only during the upload; history is kept
in Spaces, whose lifecycle must be configured separately.

## Layout

- `/opt/joppa/bin/joppa`: application executable, owned by root.
- `/etc/joppa/`: private runtime configuration, root:joppa, directory mode 0750.
- `/var/lib/joppa/`: runtime directory owned by the unprivileged service account.
- `/var/backups/joppa/`: temporary dump during upload and the backup lock,
  owned by postgres, directory mode 0700; no local backup archive.
- Caddy owns public ports 80/443; the API and PostgreSQL listen on loopback only.

Use official Ubuntu/PostgreSQL packages for PostgreSQL 16 and pgvector. Enable
`CREATE EXTENSION IF NOT EXISTS vector` as the database administrator. The
application role must not own the journal tables or run as a PostgreSQL superuser.
Apply migrations as the database owner and grant the runtime role only schema
USAGE, table SELECT/INSERT and sequence USAGE/SELECT. Only the two rebuildable
search cache tables additionally permit UPDATE/DELETE. WORM triggers remain intact.

The `postgres-2gb.conf` settings are an initial budget, not a measured capacity.
Compile release binaries with one build job, preferably outside the production
runtime. Service limits reserve memory for the database and OS.

## HTTPS and authentication

Set `JOPPA_DOMAIN` in a Caddy systemd environment override. Validate the Caddyfile
before reload. DNS must point to the target host for automatic HTTPS issuance.
The proxy preserves the browser Host and Origin used by Joppa's CSRF check.

Configure Joppa's Google client with the exact callback
`https://YOUR_DOMAIN/auth/google/callback`. Copy the private JSON described in the
root README to `/etc/joppa/google-client.json`, set `JOPPA_GOOGLE_CLIENT_FILE`, and
restart Joppa. Do not install placeholder Google credentials: missing configuration
leaves the authorization screen available but does not allow browser login.

## Data transfer and acceptance

Create a fresh custom-format dump of the local database before transfer. Restore
with `--no-owner --no-acl` into an empty database. Preserve journal positions,
request IDs, actors and timestamps. Compare workspace/event counts and the
workspace API before routing users to the new service. Do not overwrite a
nonempty target or retire the source until the restore has been verified.

`GET /health` checks database connectivity and journal schema availability.
After deployment verify HTTPS, anonymous API denial, authenticated workspace
reads, and the Google flow once the own client credentials are configured.

Install the backup service and timer after creating the backup directory.
The timer reads standard Spaces variables from `/etc/joppa/backup.env`
owned by root with mode 0600 (see `backup.env.example`). systemd supplies these
to the backup process; the application does not receive the backup credentials.
The script uploads a timestamped dump and `postgres/latest.dump` using curl
SigV4. No separate S3 client is required. The temporary dump is removed when the
script exits, including on upload failure; failed uploads leave the service failed
and existing Spaces backups unchanged. Configure Spaces lifecycle retention
separately for timestamped dumps. Test a download
and restore before calling the backup setup complete. Application credentials
need their own private, encrypted backup, separate from the database dumps.

## Application files

Attachments hold at most 50 MiB. Their bytes live in the DigitalOcean Spaces bucket
of `/etc/joppa/storage.env`, under `files/<sha256>`. PostgreSQL keeps only the
attachment record with the hash, so the database backup does not include file bytes.
The server does not start without the five variables of
[the template](storage.env.example). The server signs each Spaces request as a
presigned AWS4-HMAC-SHA256 URL: PUT and GET are valid 900 seconds, HEAD and DELETE
60 seconds. A download of `GET /api/workspaces/{workspace}/blobs/{hash}` redirects a
member to a presigned GET. The deletion of a workspace removes the files that only it
names. Backup credentials belong only to the backup service. Both environment files
are owned by root with mode 0600.

## Semantic search configuration

Use `search.env` from [the template](search.env.example) for the private DeepInfra
key. It is installed as `/etc/joppa/search.env`, root:root 0600, and loaded by
systemd. Restart the application after changing it. Indexing is asynchronous;
MCP search reports the number of current and embedded documents. Without the key
the server stops at start and names the missing variable. Embeddings are part of the
PostgreSQL backup and can also be rebuilt from journal content.

## Recovery drill

Verify a newly uploaded Spaces backup against an isolated PostgreSQL 16 instance
with pgvector. Successful `pg_restore --list` only checks the archive catalog;
it does not prove that the application can reconstruct its state.

1. Run the existing backup service and check its exit status. Download the new
   object through authenticated Spaces access into a private directory. Record
   its checksum and confirm that anonymous access is denied.
2. Restore into an empty, isolated database with `pg_restore --exit-on-error
   --single-transaction --no-owner --no-acl`. Do not restore over the production
   database or route production clients to the test instance.
3. Point the administrative server binary at the restored database. Supply the
   matching private `JOPPA_ACCESS_RULES_FILE` and `JOPPA_LOGIN_USERS_FILE` when
   checking Google-bound identities. The dump includes identity bindings but not
   these filesystem policies; missing policy deliberately denies access.
4. Read every expected workspace through the application. Compare journal
   positions, events, canonical workspace IDs, aliases, permanent item IDs and
   reconstructed cards. If production has advanced, compare the restored event
   prefix; compare full current cards only at equal journal positions.
5. Stop the isolated instance after verification. Retain a private verification
   report with backup checksum, time and restored positions. A downloaded dump
   contains private product data and credential hashes; keep it access-restricted
   under the same handling policy as the backup itself.

Back up private configuration separately in encrypted storage: Google client,
identity/access rules, service credentials, database connection configuration
and provider credentials cannot be recovered from `pg_dump`. Restore these
through secret management; never put them in Git or the public verification report.

The daily timer establishes backup frequency, not retention. Timestamped objects
expire only under a configured Spaces lifecycle rule. An API key may upload and
download backups but lack permission to inspect lifecycle settings. In that case
verify retention in the bucket's control panel; do not infer a 30-day policy from
successful uploads or remove old objects merely to achieve an assumed count.
