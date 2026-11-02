# HTTP and MCP API

Read [objects](objects.md) before you edit documents. Write new text by the
[technical writing rules](writing.md), which `joppa_help` also gives with `section: "writing"`.

## Requests

- MCP: `POST /mcp` over HTTPS. The actor comes from OAuth or an agent token.
- Commands: `POST /api/workspaces/{workspace}/commands` with `{"request_id":"unique-id","command":{...}}` and `Authorization: Bearer <token>`.
- Reads: `GET /api/workspaces`, `GET /api/workspaces/{workspace}` and the routes `board`, `objects`, `search`, `tasks`, `timeline`, `history`, `tree`, `tests` and `dependencies` under it.
- Files: `POST /api/workspaces/{workspace}/blobs` takes a raw body of 50 MiB at most and answers `{hash,size}`.

Each command has `op` with its name. IDs hold ASCII letters, digits, `-`, `_` and `.`, 120 bytes
at most. Authentication gives the actor. A request that supplies `actor` is refused.
`joppa_help` with `op` gives the required fields and the callers of one operation.
`joppa_help` without `section` or `op` gives a short table of contents: `sections`, `ops` and `hint`.

## Operations

- Objects: `save_object`, `archive_object`, `restore_object`, `set_remote_links`, `redact_object` (owner).
- Schema and order: `set_tree_schema`, `reorder_tree`, `set_priority`.
- Criteria and tests: `set_ac_tests`, `accept_commit`, `assess`, `assess_signal`.
- Discussion and files: `note`, `withdraw_note` (author only), `comment`, `attach`, `detach`.
- Planning: `claim_planning`, `finish_planning`, `release_planning`.
- Tasks: `claim_task`, `claim_next_task`, `release_task`, `block_task`, `unblock_task`, `complete_task`, `review_task`, `retire_task`.
- Workspace (owner): `rename_workspace`, `delete_workspace`, `member`, `remove_member`.

HTTP and MCP refuse every other operation. The server command line also creates a workspace
with `workspace {title}`. `joppa_record_results` records test outcomes. It is a tool, not an operation.

- `comment {req, revision, id, text, kind, reply_to, ac}` adds revision discussion. `kind` is `comment`, `decision`, `accept`, `change`, `reject` or `correction`.
- `note {target, id, text, reply_to}` adds a comment to any object.
- `attach {target, id, name, content_type, size, hash}` names bytes that the blob route stored. `joppa_attach {workspace, target, filename, content_type?, content | content_base64}` uploads and attaches in one call.
- `detach {target, id, reason}` keeps the file readable for 14 days. Then the periodic reconcile deletes it, unless an active attachment of the workspace names its hash. `joppa_attachments {workspace, target, removed: true}` also lists the files detached in the last 14 days, with `removed: true`, `removed_at` and a download `url`.
- `reorder_tree {kind, parent, expected, ids}` sets the order of siblings. `expected` must match the current order, and `ids` must hold those siblings once. `POST /api/workspaces/{workspace}/order` takes the same fields from an owner with a bearer token.
- `delete_workspace {reason}` removes every row of the workspace. Read `GET /api/workspaces/{workspace}/references` first: each workspace whose remote links or child bindings name it loses them, and the result lists them in `removed_references`.

## Readiness

The process metadata of a criterion includes `assessment.state`: `unassessed`, `waiting` or
`ready`. It also gives the specification revision, reason, recipient and decision reference.
The metadata of a Task includes the assessment of a direct criterion parent.
`readiness_blocker` names the assessment that stops a readiness gate.
A waiting Task gives the same question and recipient in `waiting_reason` and in its claim refusal.
These fields change no check evidence and no completion.

## Task queue

`joppa_tasks` and `GET /api/workspaces/{workspace}/tasks` give the `available`, `mine`,
`waiting` and `all` queues in pages. Over HTTP, send `view`, `limit`, `cursor`, `project` and
comma-separated `priorities`.

- `available` uses the rules of `claim_task`: a contract, a reviewed parent, met dependencies and the parent gate. The Task has no claim, completion or retirement.
- `claim_next_task {project?, priorities?}` selects, under the workspace lock, the oldest claimable Task of the first priority that has one, else the oldest claimable Task. `project` is a hard filter. Unknown priority targets are errors.
- A preview is `joppa_tasks` with `view=available`, `limit=1` and the same filters. A later claim can lose a race.
- A claim stays until a recorded action changes it. Time, heartbeats and restarts never release a Task.
- Only the holder completes work. The holder or the workspace owner releases it with a reason.
- An agent can hold several Tasks at the same time.
- Task status is `open`, `in_progress` or `done`. Task completion never verifies a criterion.
- `created_at` is the journal position of the Task creation, not a clock time.

`joppa_objects` with `work_filter="unstarted"` lists the criteria that need planned work.
A candidate has a ready assessment, current dependencies and a status other than green.
An active Task counts for its nearest criterion through its parents, up to the nearest
specification. Each candidate gives `work_summary` with the counts of active, done and retired
Tasks. Check `work_summary.done` before you plan new work: a done Task can need only a test run.

## Planning a criterion

Planning uses the criterion role of the schema. The type name can differ from `ac`.

- `claim_planning {ac, revision}` claims a ready criterion that is not green, at its current revision.
- `finish_planning {ac, reason}` publishes the Task set and removes the claim.
- `release_planning {ac, reason}` removes the claim and retires only its new Tasks.

Each workspace permits one planning claim per criterion and per actor. Existing Tasks do not prevent a
claim. A criterion read gives `process.planning_claim`: `null` or `{actor, position}`.
New Tasks of the holder after the claim wait with `waiting_reason="Task is awaiting planning publication"`.
They cannot be claimed until the finish.

Finish needs a Task creation or revision by the holder after the claim. It checks dependency
targets and cycles. It also compares each blocked child Task with its document at the block:
title, body, projects and blocking dependency links. An unchanged blocked Task makes finish
fail with "blocked Task needs a new contract or retirement". This refusal comes before
"planning produced no Task changes". A failed finish keeps the claim and every block.
A successful finish clears `process.blocked` of each changed blocked Task.
Only the holder finishes. The holder or the workspace owner releases with a reason.

## Workspace timeline

`GET /api/workspaces/{workspace}/timeline` and `joppa_timeline` give the same page, one entry
for each journal event, in journal order.

- Each entry has `position`, `at`, `actor`, `address`, `title`, `revision`, `change`, `op` and `reason`.
- A comment entry (op `comment`, `note` or `withdraw_note`) also has `comment_id`.
- `change` is save, move, archive, restore, comment, file, tests, readiness, priority, task, results, planning, member, schema, workspace or other. A move is a save that changes only the parent.
- A record of test results has the change `results` and the address `record/ID`.
- `after=POSITION` reads only later entries. Over HTTP, `limit` is 1 to 100, default 50. `joppa_timeline` gives at most 25 entries a page. Both give `total`.
- Send `next_cursor` as `cursor` for the next page. The last page has `next_cursor: null`. `order=newest` reads the newest entries first.
- Members and agents of the workspace read it. Another account gets 404 `Workspace not found`.

The board shows the timeline at `/?workspace=W&view=timeline`, newest first.

## Generated IDs and batches

Omit `id` when you create an object, a comment or a file. The server makes an 8-character
`[a-z0-9]` ID and returns the resolved `command`. An omitted field means a new identity.
Null or an empty string is not an ID. Keep the IDs of existing objects when you edit them.
Parents, members, owners and assignees must name existing objects or people.

A successful request stores its input and result in an immutable receipt. Retry exactly the
same input with the same `request_id` to get the same IDs. Different input or another actor
is refused. A failed transaction uses no ID and no request ID.

`joppa_batch` takes `workspace`, `request_id` and `operations` (1 to 100 entries). Each entry
has `command` and an optional unique `ref` label. The commands run in order in one
transaction: all append, or none. Each keeps its own history entry and permission check.
A later command can use a field of an earlier result with `{"$ref":"label/field"}`, for example
`requirement/data/title`. A `save_object` result also gives `address`. A forward or missing
reference fails the whole batch. Plain text is never a template.

```json
{"workspace": "product", "request_id": "create-access-tree-1", "operations": [
  {"ref": "area", "command": {"op": "save_object", "kind": "domain", "revision": 0, "parent": null,
    "data": {"title": "Access", "owner": "owner"}, "reason": "Agreed product scope"}},
  {"ref": "requirement", "command": {"op": "save_object", "kind": "requirement", "revision": 0,
    "parent": {"$ref": "area/address"}, "data": {"title": "Paid access survives signing in again",
    "body": "Only iOS is in scope", "owner": "owner"}, "reason": "Agreed product scope"}}]}
```

The reply has `results` in operation order, each with `position` and the resolved `command`.
The server command line takes explicit local IDs.

## Search

`joppa_search` takes `workspace`, `query`, `kind`, `limit` (1 to 50, default 10),
`all_versions` and `all_workspaces`. `GET /api/workspaces/{workspace}/search` takes `q` and
the same filters. See [search](search.md), also given by `joppa_help` with `section: "search"`.

## Browser authentication

Joppa has its own Google OAuth client and invitation list.

- `POST /auth/login` gives a Google authorization URL and sets an HttpOnly attempt cookie with `SameSite=Lax`.
- `GET /auth/google/callback` checks the state and the browser binding, exchanges the code with PKCE and verifies the signed ID token.
- `POST /auth/poll` gives `pending` until the end, then sets an HttpOnly `SameSite=Strict` session cookie for 12 hours.
- `POST /auth/logout` ends the session on the server. `GET /api/me` gives the actor and its verified Google email or null.

Sign-in, sign-out, polling and OAuth consent need an exact same-origin `Origin` header.
Cookies are `Secure` over HTTPS. Session token hashes and revocations stay in PostgreSQL.
A command request without an Authorization header gets 403, also with a session cookie.
The browser never stores bearer tokens. A missing Google configuration fails closed.

The board is read-only for content. The Settings page needs a browser session for its writes:
it creates a workspace, renames or deletes it, grants access and issues agent tokens.

## MCP authentication

Clients connect to `/mcp` over Streamable HTTP. A missing or bad credential gets 401 with a
`WWW-Authenticate` resource-metadata URL. `/.well-known/oauth-protected-resource/mcp` names
the authorization server, and `/.well-known/oauth-authorization-server` gives its endpoints.

- `POST /oauth/register` registers a public client with exact HTTPS or loopback redirect URIs.
- `/oauth/authorize` needs a code challenge with PKCE S256. Consent uses the Google browser sign-in.
- `/oauth/approve` needs a browser session, the exact same Origin and explicit consent. It gives a single-use code that expires after five minutes.
- `POST /oauth/token` takes form-encoded `authorization_code` or `refresh_token` exchanges.
- Access tokens last one hour. Refresh tokens last 30 days and rotate on each use. A replay revokes the whole token family.
- `POST /oauth/revoke` with `token` and `client_id` revokes the family.

The only resource is the canonical `/mcp` URL. A different resource is refused. A client that
omits it gets a token for `/mcp` only. MCP tokens are not browser cookies or REST credentials.
Notifications get 202 without a body. GET gets 405: the server needs no SSE stream.
For a client with a hosted callback that only shows the code, the consent page shows a Copy
code button. The code stays bound to the callback, the client and the PKCE verifier.

## Workspace names

`rename_workspace {title, reason, reference_prefix?, workspace_id?}` keeps all history.
A new `workspace_id` becomes the public ID in lists and reads. Old names stay aliases with
the same access, and no other workspace can take them. A new `reference_prefix` changes the
prefix of new public IDs. Old IDs stay valid.
