# Single-server deployment

Target: Ubuntu 24.04 LTS, one shared vCPU, 2 GB RAM. PostgreSQL 16 stores the
journal and will host pgvector and full-text search. DeepInfra performs embedding
inference. Joppa has its own Google OAuth client and invited account list.

These are deployment templates, not evidence that a server has been installed.
At present, vector ingestion and the hybrid search endpoint are not implemented;
installing the PostgreSQL extension alone does not enable product search.

## Layout

- `/opt/joppa/bin/joppa`: application executable, owned by root.
- `/etc/joppa/`: private runtime configuration, root:joppa, directory mode 0750.
- `/var/lib/joppa/`: runtime directory owned by the unprivileged service account.
- `/var/backups/joppa/`: database dumps, owned by postgres, directory mode 0700.
- Caddy owns public ports 80/443; the API and PostgreSQL listen on loopback only.

Use official Ubuntu/PostgreSQL packages for PostgreSQL 16 and pgvector. Enable
`CREATE EXTENSION IF NOT EXISTS vector` as the database administrator. The
application role must not own the journal tables or run as a PostgreSQL superuser.
Apply migrations as the database owner and grant the runtime role only schema
USAGE, table SELECT/INSERT and sequence USAGE/SELECT. WORM triggers remain intact.

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
Like NTK, the timer reads standard Spaces variables from `/etc/joppa/backup.env`
owned by root with mode 0600 (see `backup.env.example`). systemd supplies these
to the backup process; the application does not receive the backup credentials.
The script uploads a timestamped dump and `postgres/latest.dump` using curl
SigV4. No separate S3 client is required. It retains 14 days locally; configure
Spaces lifecycle retention separately for timestamped dumps. Test a download
and restore before calling the backup setup complete. Application credentials
need their own private, encrypted backup, separate from the database dumps.

## Application files

Use a separate private Spaces bucket and credentials for application files.
The optional `/etc/joppa/storage.env` follows `storage.env.example` and is loaded
only by the application service. Backup credentials belong only to the backup
service. Both environment files are owned by root with mode 0600. Verify bucket
scopes: a different key ID alone does not prove separation of permissions.
The storage configuration is ready for the attachment implementation; application
upload endpoints are not yet implemented.
