# Single-server deployment

Target: Ubuntu 24.04 LTS, one shared vCPU, 2 GB RAM. PostgreSQL 16 stores the
journal and will host pgvector and full-text search. DeepInfra performs embedding
inference. Joppa has its own Google OAuth client and invited account list.

At present, vector ingestion and the hybrid search endpoint are not implemented;
installing the PostgreSQL extension alone does not enable product search.

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
`backup.env` and `storage.env`. Only those five files are transferred. Optional
files omitted on an update keep their existing server configuration. Without a
Google client on the server, the authorization page loads but sign-in stays disabled.

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

Use a separate private Spaces bucket and credentials for application files.
The optional `/etc/joppa/storage.env` follows `storage.env.example` and is loaded
only by the application service. Backup credentials belong only to the backup
service. Both environment files are owned by root with mode 0600. Verify bucket
scopes: a different key ID alone does not prove separation of permissions.
The storage configuration is ready for the attachment implementation; application
upload endpoints are not yet implemented.
