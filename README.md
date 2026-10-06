# joppa

**A versioned product tree with connected tasks.**

Each workspace defines its object types, parents, links, colors and order.
The default recipe is Domain → Capability → Requirement → AC → Task.
Tasks belong to the parents that the workspace schema permits.
The queue selects the next eligible task from priorities and dependencies.
One workspace is one product, even when several repositories deliver it.
Repository references follow the workspace schema.

People and agents formulate REQ and ACs together. The REQ owner confirms wording
and accepts the checked result. Agents assess each AC: independent ready
conditions can proceed while disputed ones wait for a named decision maker.
An iOS condition means iOS; other clients are never added by assumption.

## What works in 0.2

- Workspaces, members, decision authorities, capabilities and owned REQs/ACs.
  Agents propose the tree; the product owner confirms it.
- Incoming Notes with revisions, comments, attachments and links to affected objects.
  The default queue shows unprocessed Notes. Processed Notes stay available.
- REQ cards with revisions, discussion, specialist decisions and actor/time
  history. Every AC in a new revision starts at **n/a**.
- Tasks under configured object parents, manual work and decision tasks. A completed
  task is not a verified AC. Manual checks store exact expected/observed lists.
- Sealed check plans, evidence, owner acceptance, withdrawn decisions, confirmed
  defects and launch conditions. Records append; history is never rewritten.

MCP supports generated creation IDs, atomic batches and English methodology
without a local checkout. Agents can clarify unconfirmed domain/capability
titles while approval and changes of authority remain with the owner.

## Run locally for development

Use Rust 1.88+, PostgreSQL 16 with pgvector and Python 3 for examples/check adapters.
If a local PostgreSQL container does not already exist:

```sh
docker run -d --name joppa-db -e POSTGRES_PASSWORD=joppa \
  -p 127.0.0.1:55433:5432 -v joppa-data:/var/lib/postgresql/data pgvector/pgvector:pg16
# Wait until pg_isready reports accepting connections:
docker exec joppa-db pg_isready -U postgres
docker exec joppa-db createdb -U postgres joppa
```

For an existing database, start here. Migration adds the `joppa` schema without
removing or importing legacy `public` tables.
Create new workspaces only with the server administrative CLI; HTTP API and MCP
can edit existing workspaces but cannot create them.

```sh
cargo build --locked
export JOPPA_DATABASE_URL=postgresql://postgres:joppa@127.0.0.1:55433/joppa
./target/debug/joppa migrate
export JOPPA_ACTOR=owner
printf '%s\n' '{"op":"workspace","title":"My product"}' | \
  ./target/debug/joppa command product create-product
```

Create a private service credential map for API/admin integrations:

```sh
python3 - <<'PYCONFIG'
import json, os, secrets
path = '/tmp/joppa-tokens.json'
with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as f:
    json.dump({secrets.token_urlsafe(32): 'owner'}, f)
print('API credentials saved to', path)
PYCONFIG
export JOPPA_TOKENS_FILE=/tmp/joppa-tokens.json
./target/debug/joppa serve 6212
```

Browser sign-in talks directly to Google using Joppa's own OAuth client
and invitation list.

Before starting `serve`, set `JOPPA_GOOGLE_CLIENT_FILE` to a private JSON file:

```json
{
  "client_id": "YOUR_JOPPA_CLIENT.apps.googleusercontent.com",
  "client_secret": "YOUR_JOPPA_CLIENT_SECRET",
  "redirect_uri": "https://YOUR_JOPPA_DOMAIN/auth/google/callback"
}
```

Register that exact callback in Joppa's Google Cloud OAuth client. HTTPS is
required outside localhost. Set `JOPPA_LOGIN_USERS_FILE` to a private JSON file
mapping invited Google email addresses to existing Joppa actors, for example
`{"owner@example.com": "owner"}`. Each actor must exist in the API identity file.
Files containing secrets must be readable only by the service account.

For automatic workspace assignment, set `JOPPA_ACCESS_RULES_FILE` to a private
JSON file following [the access rules example](server/access-rules.json.example).
A verified organization domain assigns its workspace list. An exact email rule
overrides that list; `[]` denies access. Domain rules require Google's verified
organization claim, not just the suffix of an email address. Personal accounts
need explicit email rules. Rules are checked on protected requests, so revocation
also applies to existing sessions. New grants enroll members at their next sign-in.
The journal preserves membership history; current access rules remain authoritative.
Keep real domains, addresses and workspace assignments out of Git.

Open Joppa and choose **Continue with Google**. Complete sign-in in the Google
window; the board opens automatically. Until sign-in, only the authorization
screen is visible. The server verifies the token signature, audience, issuer,
expiry, nonce and email verification before applying Joppa's own invitation list.
OAuth uses a browser-bound, single-use state and PKCE. Browser sessions have
separate HttpOnly cookies and expire after 12 hours. Session token hashes and
sign-out revocations are stored in PostgreSQL, so deployments preserve sign-in. Google credentials and API tokens never reach browser storage.
Without Joppa's own Google configuration, sign-in fails closed with a setup message.

The **Workspace** menu lists only accessible products. Root tabs come from each
workspace schema. Type titles, colors and order follow that schema.
All object types use the same card. Open a card to read its document and direct
children. Owner and status filters apply within the selected root tab.
The initial page contains metadata. It loads documents and earlier revisions
only when requested. Closing and opening a loaded card uses its cached content.

Use the **copy + robot** button on any card to copy its permanent identifier.
A success mark and a toast confirm copying. Agents resolve the copied identifier
with `joppa_read({"item_id":"..."})`. The same reader handles every configured type.
Use **Revisions** to read one saved document. Historical documents omit current
comments, files and process status. Current comments, attachments and Context
remain separate annotations.

The portal is read-only. Agents change documents, links and schema through MCP.
Types with `inbox: true` supply an input queue. It shows unprocessed inputs first.
The **All** checkbox includes processed inputs. Related inputs remain visible
under the objects they affect. Processing preserves the source document.

If port 6212 is occupied, use another local port such as 6213. Public access
requires an HTTPS proxy; see [server deployment](server/README.md).

## Connect MCP

**One endpoint: `https://YOUR_JOPPA_DOMAIN/mcp`.**
Use HTTP (Streamable HTTP). No local Joppa client binary is needed. Add the server,
start authentication, sign in with Google and approve access. The client stores
and refreshes its OAuth credentials.

In agy: `/mcp` → joppa → Authenticate. After approval Joppa displays **Copy code**;
paste it into agy. The external Antigravity callback page is not needed.

For a localhost callback, Joppa displays **Copy sign-in link**. Paste the entire
link into your client's pending authorization prompt, including `code` and
`state`. Clients with a local callback listener can use **Return to app**.
The link is single-use and expires after five minutes; start authentication
again if it expires. Hosted HTTPS callbacks still return directly to the client.

Claude Code, globally:

```sh
claude mcp add --transport http --scope user joppa https://YOUR_JOPPA_DOMAIN/mcp
```

Codex, in the user's `config.toml`:

```toml
[mcp_servers.joppa]
url = "https://YOUR_JOPPA_DOMAIN/mcp"
```

Then run `codex mcp login joppa` in a terminal with the same Codex configuration.
Complete browser sign-in and choose **Return to app**, then reconnect MCP.
The browser must be able to reach Codex's local callback; containers need that
callback exposed. In the Codex app, use MCP server settings → **Authenticate**.

agy, globally:

```sh
agy mcp add --type http joppa https://YOUR_JOPPA_DOMAIN/mcp
```

OpenCode 1.x, in the global `opencode.jsonc`:

```json
{"mcp":{"joppa":{"type":"remote","url":"https://YOUR_JOPPA_DOMAIN/mcp","enabled":true}}}
```

| Tool | Purpose |
|---|---|
| `joppa_list` | List accessible products |
| `joppa_read` | Read a workspace or resolve a copied system ID |
| `joppa_tasks` | List available, assigned or waiting Tasks without opening every REQ |
| `joppa_search` | Search current wording inside one authorized workspace |
| `joppa_model_docs` | Read English model, process and evidence rules |
| `joppa_help` | Read command shapes and API rules |
| `joppa_tree` | Read the workspace schema and its saved revisions |
| `joppa_objects` | Read object lists, documents, links and revisions |
| `joppa_command` | Append one operation; omit IDs for new objects |
| `joppa_batch` | Append up to 100 related operations atomically |

For Bash scripts, [client/joppa](client/joppa) calls the same MCP endpoint.
It needs Python 3 and a private service bearer for an enrolled agent member;
the server's administrative executable is not installed on the client machine.

```sh
export JOPPA_MCP_URL=https://YOUR_JOPPA_DOMAIN/mcp
export JOPPA_TOKEN_FILE=/private/path/to/agent-token
export JOPPA_WORKSPACE=product
python3 client/joppa task list --view available
python3 client/joppa task next --purpose implementation --request-id "$NEXT_REQUEST_ID"
python3 client/joppa task next --priority domain:billing --priority requirement:r-123
python3 client/joppa task next --dry-run --priority domain:billing
python3 client/joppa task claim TASK_ID --request-id "$CLAIM_REQUEST_ID"
python3 client/joppa task complete TASK_ID --evidence 'merge abc123'
```

Commands print JSON. `next` selects and claims under one server lock. Repeat
`--priority kind:id` in preference order. Each priority addresses an object from
the configured tree. Reference scopes follow the workspace schema. It tries the first scope with a
claimable Task, then the next, then the ordinary oldest Task. `--project` and
`--purpose` remain hard filters. `next --dry-run` returns `{ "task": ... }`
without claiming; `task` is `null` when nothing is available. Another worker
must atomically `claim` the returned `id`, `req` and `revision`; it may lose a
race and should retry the preview. `claim`
accepts the Task ID from the queue; use `--req REQ --revision N` when an ID is
ambiguous. Reuse a `--request-id` only for an identical retry. Exit code 3 means
the Task was already claimed or no matching Task is available; other errors
return 1. Do not pass the bearer token on the command line.

Each write needs a unique `request_id`. Retry identical input with the same ID
for the same result, including generated IDs. Batches can reference earlier
created objects without client-generated slugs. See [API examples](docs/api.md#automatic-ids-and-batches).

OAuth uses client registration, authorization codes and PKCE S256. Access tokens
last one hour. Refresh tokens last 30 days and rotate on use; reuse revokes the
entire family. Tokens are restricted to `/mcp`, stored server-side only as hashes
and survive deployment. Workspace access is checked for every operation.
The public origin comes from the Google callback, or explicit `JOPPA_PUBLIC_URL`.
Only loopback development origins may use HTTP.

An interactive agent acts as your account. It must confirm wording, record
human decisions and accept results only on explicit instruction. Unattended
automation uses a separate `agent` identity and private service bearer token.

The server Rust executable handles serving, migrations and administration. It
is not installed to user machines and does not expose stdio MCP. Administrative
commands require `JOPPA_DATABASE_URL` and `JOPPA_ACTOR`; do not give those powers
to user agents. `gate WORKSPACE < subject.json` remains a server-side release
check requiring acceptance for specified builds and configurations.

## Automated checks

Put the AC's local ID in the test name: `[ios-access] paid access survives login`.
One test names one AC; an AC can have several tests. Submit the complete expected
set and a discovery-complete assertion before execution.

```sh
JOPPA_ACTOR=ci python3 scripts/run_checks.py product examples/checks.json \
  --joppa ./target/debug/joppa
```

Replace sample component versions and commands with real ones. The adapter seals
the plan, executes commands without a shell, records each result and completes
the run. Submit mixed manual/automated plans through the API. Missing/skipped
checks and incomplete discovery cannot turn green. People still judge the
meaning and sufficiency of tests.

## Storage and upgrading from 0.1

`migrations/*.sql` is the schema source; `schema.sql` invokes the same files for
psql. Migrations are repeatable. `joppa.events` stores immutable typed commands,
authenticated actors and server time. State is replayed from the journal. Writes
serialize within a workspace, and REQ changes also check `expected_revision`.

Triggers reject journal UPDATE, DELETE and TRUNCATE. Run the service with
SELECT/INSERT and sequence access, without table/schema ownership; see
[storage](docs/storage.md). A PostgreSQL superuser can disable triggers, so
physical immutable archiving is separate infrastructure.

`joppa legacy serve 6214` exposes the retained 0.1 board. It is not a write path
for the new model. Migration does not pretend old `done_at` flags are checks,
guess owners or confirm actual business rules.

## Verify

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
docker exec joppa-db createdb -U postgres joppa_test_local
JOPPA_TEST_DATABASE_URL=postgresql://postgres:joppa@127.0.0.1:55433/joppa_test_local \
  cargo test --test postgres --test references --test authoring -- --ignored
```

Integration tests require an isolated database whose name starts with
`joppa_test`. They exercise concurrent changes/results/acceptance, workspace
isolation, retries, migration repeatability, preserved legacy tables and WORM.
CI runs PostgreSQL 16 with pgvector and the HTTP, OAuth, session, search and access checks too.

## Current boundaries

The working cycle is formulate → decide → implement → check → accept.
Repository-specific CI integration and business-data migration need product
onboarding and owner review. There is no background task executor, automatic
legal-policy interpretation or proof that an AC set is complete.

Agent search combines PostgreSQL full-text and pgvector results when the private
DeepInfra key is configured; otherwise it reports full-text mode. See [search](docs/search.md).
Attachment upload endpoints are not yet implemented; their storage configuration
is preparatory. Current object lists and board reads use database projections. Full journal
reads and writes still replay workspace history. The portal is read-only; authoring and decisions
use MCP.

See the [model](docs/model.md), [process](docs/process.md), [green rule](docs/green.md),
[historical review](docs/reviews.md) and [implemented fixes](docs/implementation.md).

Product follow-up work and acceptance criteria: [work queue](docs/roadmap.md).
