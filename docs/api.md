# HTTP and MCP API

Read [objects](objects.md) before you edit documents. Write new text by the
[technical writing rules](writing.md), which `joppa_help` also gives with `section: "writing"`.

## Requests

- MCP: `POST /mcp` over HTTPS. The actor comes from OAuth or an agent token.
- Commands: `POST /api/workspaces/{workspace}/commands` with `{"request_id":"unique-id","command":{...}}` and `Authorization: Bearer <token>`.
- Reads: `GET /api/workspaces`, `GET /api/workspaces/{workspace}` and the routes `board`, `objects`, `search`, `timeline`, `history`, `tree` and `dependencies` under it.
- Files: `POST /api/workspaces/{workspace}/blobs` takes a raw body of 50 MiB at most and answers `{hash,size}`.

Each command has `op` with its name. IDs hold ASCII letters, digits, `-`, `_` and `.`, 120 bytes
at most. Authentication gives the actor. A request that supplies `actor` is refused.
`joppa_help` with `op` gives the required fields and the callers of one operation.
`joppa_help` without `section` or `op` gives a short table of contents: `sections`, `ops` and `hint`.

## Operations

- Objects: `save_object`, `archive_object`, `restore_object`, `set_remote_links`, `redact_object` (owner).
- Schema and order: `set_tree_schema`, `reorder_tree`.
- Criteria and tests: `save_object` with a test node, `accept_commit`, `assess`.
- Discussion and files: `note`, `withdraw_note` (author only), `attach`, `detach`.
- Workspace (owner): `rename_workspace`, `delete_workspace`, `member`, `remove_member`.

HTTP and MCP refuse every other operation. The server command line also creates a workspace
with `workspace {title}`. `joppa_record_results` records test outcomes. It is a tool, not an operation.

- `note {target, id, text, reply_to}` adds a comment to any object.
- `attach {target, id, name, content_type, size, hash}` names bytes that the blob route stored. `joppa_attach {workspace, target, filename, content_type?, content | content_base64}` uploads and attaches in one call.
- `detach {target, id, reason}` keeps the file readable for 14 days. Then the periodic reconcile deletes it, unless an active attachment of the workspace names its hash. `joppa_attachments {workspace, target, removed: true}` also lists the files detached in the last 14 days, with `removed: true`, `removed_at` and a download `url`.
- `reorder_tree {kind, parent, expected, ids}` sets the order of siblings. `expected` must match the current order, and `ids` must hold those siblings once. `POST /api/workspaces/{workspace}/order` takes the same fields from an owner with a bearer token.
- `delete_workspace {reason}` removes every row of the workspace. Read `GET /api/workspaces/{workspace}/references` first: each workspace whose remote links or child bindings name it loses them, and the result lists them in `removed_references`.

## Readiness

`assess {target, ready, reason}` sets the readiness mark of any present node.
The process metadata of the node then gives `assessment`: `{state, reason, actor, position}`.
`state` is `ready` or `waiting`. The node filter `ready` is false for a waiting node.
The mark changes no check evidence and no acceptance. A change that resets a criterion also
removes its mark.

`expected_position` is optional. Send the `position` of the mark that you read, or 0 when the
node has no mark. When the current mark has another position, the write is refused with
"stale assessment; read the object again". In `joppa_batch`, the refusal stops every operation
of the batch:

```json
{"op": "assess", "target": "ac:a", "ready": true, "reason": "Retry passed", "expected_position": 812}
```

Live writes refuse the form with `req`, `revision` and `ac`:
"assess with req, revision and ac was removed; use assess {target, ready, reason}".

## Tasks

A Task is a node like any other. See [objects](objects.md#tasks).

Live writes refuse the operations `claim_task`, `claim_next_task`, `heartbeat_task`, `release_task`,
`block_task`, `unblock_task`, `complete_task`, `review_task`, `retire_task`, `claim_planning`,
`finish_planning`, `release_planning`, `set_priority`, `assess_signal` and `comment`.
The refusal is "<op> was removed; use <replacement>". The replacement of `comment` is `note`.

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

## Board views

The header drop-down Board view (Tree or Graph) changes the board view and keeps the node
filters of the link. Every board list reads one page of 25 rows. Load more reads the next page
with its cursor. Each filter field of the tree schema (`filters`, see
[schema](schema.md#filter-fields)) adds one drop-down to the filter bar. A choice is a part of
the `data` node filter.

- Tree, `/?workspace=W`: the root collections, the cards and their children.
- Graph, `/?workspace=W&view=graph`: one SVG of every present node in a live force layout.
  The graph reads the nodes with
  `GET /api/workspaces/{workspace}/objects?parent=*&limit=100&fields=kind,parent,title,signal,links`.
  - A dot has the colour of its status tone. Its size shows its level: a root is the
    largest. Two dots never overlap, and two shown labels never overlap.
  - A line joins a parent to its child. A dashed arrow goes from a node to each node that
    `depends_on` it.
  - A graph of more than 120 nodes shows the top 3 levels first. Detail depth (the `depth`
    node filter) shows the top 2 levels or all levels.
  - A filter, a field choice, the Object root picker or a search keeps its subgraph: the
    matches, the nodes below them and their `depends_on` nodes up and down. Every other
    node hides.
  - The flow of a clicked dot, or of a subgraph of at most 24 nodes, is lit: its
    `depends_on` nodes up to 2 hops away, in flow layers with step numbers and a trail. The
    other nodes fade to opacity 0.08. A card shows the node, its parent, its data fields, the
    start of its body and the nodes one step up and down. Focus flow keeps only that flow,
    and a click on the empty canvas resets it.
  - A drag of a dot pulls its linked dots. A drag from any other place pans. A wheel or a
    two-finger swipe pans, and a pinch (a wheel with Ctrl or Cmd, or a Safari gesture) zooms.

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

Each field of a write that names an object takes its address or its public item ID, such as
`joppa-R-DP6KXM62`: `target`, `parent`, `duplicate_of`, `links[].target`, each reference
field of `data` (one ID or a list) and the `id` of `save_object`, which must name an object
of its `kind`. `joppa_history_mark` takes IDs in `nodes`. The links of `set_remote_links`
name objects of other workspaces, so they need addresses. The server replaces each ID with the `kind:id`
address before the validation, so the journal, the documents and the marks store addresses.
An unknown ID is refused: `unknown object: <ID>`.

An optional `defaults` gives `save_object` fields, for example `reason` and `data.owner`.
Each `save_object` of the batch takes each default field that it omits.
`data` merges by key, and a field of the command wins. Other operations stay as sent.
`defaults` with `op`, `actor` or a `data` that is not an object is refused. The receipt
stores the completed commands, so an identical retry gives the same reply.

```json
{"workspace": "product", "request_id": "tests-1",
 "defaults": {"reason": "Bind the tests", "data": {"owner": "owner"}},
 "operations": [
  {"command": {"op": "save_object", "kind": "test", "revision": 0, "parent": "ac:x",
    "data": {"title": "tests/a.rs > first"}}},
  {"command": {"op": "save_object", "kind": "test", "revision": 0, "parent": "ac:x",
    "data": {"title": "tests/a.rs > second"}}}]}
```
The server command line takes explicit local IDs.

## Search

`joppa_search` takes `workspace`, `query`, `limit` (1 to 25, default 10), `all_versions`,
`all_workspaces` and the [node filters](read-contract.md#node-filters) of `joppa_objects`. `GET /api/workspaces/{workspace}/search` takes `q`,
`cursor` and the same filters. See [search](search.md), also given by `joppa_help` with `section: "search"`.

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
