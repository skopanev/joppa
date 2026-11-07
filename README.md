# joppa

**A versioned product tree with connected tasks.**

Each workspace defines its object types, parents, links, colors and order.
The default recipe is Domain → Capability → Requirement → AC / TAC → Task / Test.
Tasks and test nodes belong to the parents that the workspace schema permits (`ac` or `tac`).
One workspace is one product, even when several repositories deliver it.
Repository references follow the workspace schema.

People and agents formulate REQ, ACs and TACs together. The REQ owner confirms wording
and accepts the checked result. Agents analyze each criterion. Independent
conditions can proceed while disputed ones wait for a named decision maker.
An iOS condition means iOS; other clients are never added by assumption.

## What works

- Workspaces, members, decision authorities, capabilities and owned REQs, ACs and TACs.
- Incoming Notes with revisions, comments, attachments and links to affected objects.
  The default queue shows unprocessed Notes. Processed Notes stay available.
- REQ cards with revisions, discussion, specialist decisions and actor/time
  history.
- Each test is a node under its criterion (`ac` or `tac`): `save_object` with kind `test`.
  `joppa_record_results` records the outcomes of one commit.
- No criterion, no test: every test in the repository has a present test node. A
  full-run record must give `"unbound": []` before a merge.
- Each type has a status rule: `own`, `children`, `links` or `none`. One rollup
  gives every level the highest tone of its children: red, orange, yellow, grey,
  green. A criterion without a test node is orange.
- Tasks under configured object parents, manual work and decision tasks. A Task is not
  a verified AC. Owner acceptance stays separate. Records append;
  history is never rewritten.

MCP supports generated creation IDs, atomic batches and English methodology
without a local checkout.

## Run locally for development

Use Rust 1.90+, PostgreSQL 16 with pgvector and Python 3 for the tests.
If a local PostgreSQL container does not already exist:

```sh
docker run -d --name joppa-db -e POSTGRES_PASSWORD=joppa \
  -p 127.0.0.1:55433:5432 -v joppa-data:/var/lib/postgresql/data pgvector/pgvector:pg16
# Wait until pg_isready reports accepting connections:
docker exec joppa-db pg_isready -U postgres
docker exec joppa-db createdb -U postgres joppa
```

For an existing database, start here. Migration adds the `joppa` schema.
Create a workspace with the server command line or on the **Settings** page.
Commands and MCP cannot create a workspace.

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
User labels prefer the verified Google profile name. If it is absent, labels
use the email name or the actor name. Sign in again to save a profile name.
The initial page contains metadata. It loads documents and earlier revisions
only when requested. Closing and opening a loaded card uses its cached content.

Use the **copy** button on any card to copy its permanent identifier.
A success mark and a toast confirm copying. Agents resolve the copied identifier
with `joppa_search`: the reply gives only that object, with `exact_id: true`. The same search handles every configured type.
Use **Revisions** to read one saved document. Historical documents omit current
comments, files and process status. Current comments, attachments and Context
remain separate annotations.

The board is read-only for content. Agents change documents, links and schema through MCP.
The **Settings** page creates workspaces, grants access and issues agent tokens.
Types with `inbox: true` supply a Note queue. It shows unprocessed Notes first.
The **All** checkbox includes processed Notes. Related Notes stay visible
under the objects they affect. Processing keeps the source document.

If port 6212 is occupied, use another local port such as 6213. Public access
requires an HTTPS proxy; see [server deployment](server/README.md).

## Connect MCP

**One endpoint: `https://YOUR_JOPPA_DOMAIN/mcp`.** Our instance: `https://joppa.otion.us/mcp`.
Use HTTP (Streamable HTTP). No local Joppa client binary is needed. For example, Claude Code:

```sh
claude mcp add --transport http --scope user joppa https://YOUR_JOPPA_DOMAIN/mcp
```

See [connect MCP](docs/connect.md) for other clients, agent tokens, the tool list and scripts.

## Automated checks

Save one test node under the criterion for each test: `save_object` with kind
`test` and the test name as `data.title`. Send each run with `joppa_record_results`:
the commit and one result for each test. The workspace schema gives the status
names and the rules that make a criterion green or red. Bind every new or renamed
test to its AC in the same change. See [what green means](docs/green.md) and
[the workspace schema](docs/schema.md).

## Storage

`migrations/*.sql` is the schema source; `schema.sql` invokes the same files for
psql. Migrations are repeatable. `joppa.events` stores immutable typed commands,
authenticated actors and server time. Current state uses separate object rows. Writes
serialize within a workspace, and each document write checks the current `revision`.

Triggers reject journal UPDATE, DELETE and TRUNCATE. Run the service with
SELECT/INSERT and sequence access, without table/schema ownership; see
[storage](docs/storage.md). A PostgreSQL superuser can disable triggers, so
physical immutable archiving is separate infrastructure.

## Verify

Use the public Bun commands. They hold the shared Rust lock.
The gate uses fresh test databases and runs all Rust and browser tests.

```sh
bun run gate /absolute/source/.local/lane-test.env
bun run test:only test_name_substring timeline_http
```

`test:only` runs matching Rust tests on a fresh database. It fails when no tests match.
Name the test file (`timeline_http` for `tests/timeline_http.rs`) as the second
argument. Then Cargo builds only that test, not every test binary: about 26 s instead
of about 300 s after a source change. Use `lib` for a unit test in `src/lib.rs` and `bin` for one in `src/main.rs`
(such as `web::tests`). The target is required, so a focused run never builds every test.
The gate runs the Rust suite in two workers, each with its own database, and then
the browser files on a third database. `test:browser` is the public browser runner.
Set `JOPPA_LANE_TEST_ENV` to the private environment file when the default path does not apply.
The file sets `JOPPA_TEST_ADMIN_URL` for the disposable test server. Keep it outside Git.
Record test results with the MCP tool `joppa_record_results`: one call holds the commit
and the outcome of each test. It is the only way to write test results.
Each invocation drops only its own test database when it exits.
The build wrapper clears only its private `target` directory when the `Cargo.lock` fingerprint changes.
It logs shared lock waiting in seconds. Browser-only commands do not clear compiled artifacts.

Integration tests require an isolated database whose name starts with
`joppa_test`. They exercise concurrent changes/results/acceptance, workspace
isolation, retries, migration repeatability and WORM.
CI runs PostgreSQL 16 with pgvector and the HTTP, OAuth, session, search and access checks too.

## Current boundaries

The working cycle is formulate → decide → implement → check → accept.
Repository-specific CI integration and business-data migration need product
onboarding and owner review. There is no background task executor, automatic
legal-policy interpretation or proof that an AC set is complete.

Agent search combines PostgreSQL full-text and pgvector results when the private
DeepInfra key is configured; otherwise it reports full-text mode. See [search](docs/search.md).
Object lists and selected documents use database projections.
Writes update changed rows in the same transaction as their events.
Administrative recovery replays the journal. The board is read-only for content; authoring and
decisions use MCP.

See the [model](docs/model.md), [process](docs/process.md), [objects](docs/objects.md),
[workspace schema](docs/schema.md), [links](docs/links.md), [green rule](docs/green.md),
[API](docs/api.md), [read contract](docs/read-contract.md) and [storage](docs/storage.md).
