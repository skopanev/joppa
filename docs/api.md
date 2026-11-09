# HTTP and MCP API

Read [objects](objects.md) before you edit documents. Write new text by the
[technical writing rules](writing.md), which `joppa_help` also gives with `section: "writing"`.

## Requests

- MCP: `POST /mcp` over HTTPS. The actor comes from OAuth or a private service credential of the server. An agent names itself with `alias`.
- Commands: `POST /api/workspaces/{workspace}/commands` with `{"request_id":"unique-id","command":{...}}` and `Authorization: Bearer <token>`.
- Reads: `GET /api/workspaces`, `GET /api/workspaces/{workspace}` and the routes `board`, `objects`, `search`, `history`, `tree` and `dependencies` under it.
- Files: `POST /api/workspaces/{workspace}/blobs` takes a raw body of 50 MiB at most and answers `{hash,size}`.
- Upload links: `PUT /api/uploads/{token}` takes the raw bytes of one `joppa_attach` call with `upload: true`. The signed link is the credential, so the request needs no token or cookie. The server checks the signature, the lifetime, the access and the size in this order. Then it stores the bytes and answers with the `joppa_attach` reply. The errors are:
  - 403 `upload link is not valid`, 410 `upload link expired` and 404 `Workspace not found`.
  - 413 `an attachment is at most 50 MiB`, and 400 `the upload did not finish` when the body stops before its end.
  - 409 `an empty file carries nothing` for an empty body, and 409 `request_id reused with different content/actor` when the link already stored other bytes.

Each command has `op` with its name. IDs hold ASCII letters, digits, `-`, `_` and `.`, 120 bytes
at most. Authentication gives the actor. A request that supplies `actor` is refused.
`joppa_help` with `op` gives the required fields and the callers of one operation.
`joppa_help` without `section` or `op` gives a short table of contents: `sections`, `ops` and `hint`.

## Operations

- Objects: `save_object`, `archive_object`, `restore_object`, `set_remote_links`, `redact_object` (owner).
- Schema and order: `set_tree_schema`, `reorder_tree`.
- Criteria and tests: `save_object` with a test node.
- Discussion and files: `note`, `withdraw_note` (author only), `attach`, `detach`.
- Workspace (owner): `update_workspace`, `delete_workspace`, `member`, `remove_member`.

HTTP and MCP refuse every other operation. The server command line also creates a workspace
with `workspace {title}`. `joppa_record_results` records test outcomes. It is a tool, not an operation.

- `note {target, id, text, reply_to}` adds a comment to any object.
- `attach {target, id, name, content_type, size, hash, check_result?, valid_until?}` names bytes that the blob route stored. `joppa_attach {workspace, target, filename, content_type?, content | content_base64 | url | upload, check_result?, valid_until?}` uploads and attaches in one call. Send exactly one of `content`, `content_base64`, `url` or `upload: true`.
  - `url` is an HTTPS GET with at most 3 redirects, 50 MiB and 30 seconds. The server refuses a url with a user or a password. It refuses a host with any private, loopback, link-local, shared, reserved, documentation or multicast address, also inside an IPv6 address. It connects only to the addresses that it checked. `content_type` defaults to the Content-Type of the reply.
  - `upload: true` checks the access and the fields, stores nothing and gives `{upload_url, expires_at}`. `upload_url` is `<public origin>/api/uploads/<token>`. The public origin is `JOPPA_PUBLIC_URL`, else the origin of the Google redirect URI. The link is valid 15 minutes. One PUT of the raw bytes to it attaches the file, for example `curl -T file.pdf "$upload_url"`. A restart of the server makes older links invalid.
  - `check_result` (`pass` or `fail`) makes the file a manual check of a criterion, and `valid_until` (`YYYY-MM-DD`, UTC) ends a pass.
- `detach {target, id, reason}` keeps the file readable for 14 days. Then the periodic reconcile deletes it, unless an active attachment of the workspace names its hash. `joppa_attachments {workspace, target, removed: true}` also lists the files detached in the last 14 days, with `removed: true`, `removed_at` and a download `url`.
- `reorder_tree {kind, parent, expected, ids}` sets the order of siblings. `expected` must match the current order, and `ids` must hold those siblings once. `POST /api/workspaces/{workspace}/order` takes the same fields from an owner with a bearer token.
- `delete_workspace {reason}` removes every row of the workspace. Read `GET /api/workspaces/{workspace}/references` first: each workspace whose remote links or child bindings name it loses them, and the result lists them in `removed_references`.

## No manual acceptance

There is no manual acceptance. Record the owner decision as a Source. Green Test nodes prove
the criteria. MCP, `POST /commands` and the server command line refuse `accept`, `revoke` and
`accept_commit`:
`<op> was removed; manual acceptance is gone: record the owner decision as a Source and prove it with tests`.

## Work state

The state of a node is a typed field, such as `stage`. Set it with `save_object`.
A parent can compute it with `from: children` or `from: links`.
See [computed fields](schema.md#computed-fields).

## Tasks

A Task is a node like any other. See [objects](objects.md#tasks).

Live writes refuse the operations `claim_task`, `claim_next_task`, `heartbeat_task`, `release_task`,
`block_task`, `unblock_task`, `complete_task`, `review_task`, `retire_task`, `claim_planning`,
`finish_planning`, `release_planning`, `set_priority`, `assess`, `assess_signal` and `comment`.
The refusal is "<op> was removed; use <replacement>". The replacement of `comment` is `note`.

## History

`GET /api/workspaces/{workspace}/history` and `joppa_history` give the same page, one entry
for each journal event.

- Each entry has `position`, `at`, `actor`, `address`, `title`, `revision`, `change`, `op`, `reason` and `status`.
- `status` is `open` until a member marks the entry processed with `joppa_history_mark`. A processed entry also has `processed`: `actor`, `at`, `reason` and `nodes`.
- A comment entry (op `comment`, `note` or `withdraw_note`) also has `comment_id`.
- `change` is save, move, archive, restore, comment, file, tests, readiness, priority, task, results, planning, member, schema, workspace or other. A move is a save that changes only the parent.
- A record of test results has the change `results` and the address `record/ID`.
- `order=newest` reads the newest entries first; the default is `oldest`. `after=POSITION` reads only later entries. `open=true` gives only the open entries.
- `limit` is 1 to 25, default 25; a larger limit is clamped. `total` counts the entries of the read after `after`. `next_item` names the oldest open entry.
- Send `next_cursor` as `cursor` for the next page. The last page has `next_cursor: null`.
- Members and agents of the workspace read it. Another account gets 404 `Workspace not found`.
- Over HTTP an entry also has `signal`, the current status of its object.

The board shows History as a tab beside the root tabs, newest first, with the switches
"Open only" and "Oldest first". A row opens its object at the revision of the change.

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
Parents, members and owners must name existing objects or people. `data.assignee` takes a
member ID or an agent alias, such as `lane3`.

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
    "data": {"title": "A refund of 40 EUR at the limit is paid", "case": "boundary",
     "body": "Inputs: a limit of 40 EUR and a refund of 40 EUR. Expected: the refund is paid."}}},
  {"command": {"op": "save_object", "kind": "test", "revision": 0, "parent": "ac:x",
    "data": {"title": "A refund of 50 EUR over the limit is refused", "case": "off_nominal",
     "body": "Inputs: a limit of 40 EUR and a refund of 50 EUR. Expected: the refund is refused."}}}]}
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
it creates a workspace, renames or deletes it and grants access. Each member also hides a
workspace there from the own list of that member.

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

## Agent alias

Several agents of one person share the OAuth session of that person. Each MCP tool takes the
argument `alias`, such as `p1`. The header `X-Joppa-Alias` gives the alias of every call of a
session, and the argument wins. An alias has 1 to 40 letters, digits, `-`, `_` or `.`.

- The alias changes no access: the actor of the session keeps every right.
- The journal stores it with each event (`joppa.events.alias`), and a History mark stores it too.
- History, the revision headers, comments and files give
  `alias` next to `actor`. A rebuild from the journal keeps every alias.
- The board shows the account and the alias, for example `alex · p1`.

## Workspace names

`update_workspace {title?, workspace_id?, reference_prefix?, hidden?, reason?}` changes each
field that it sends and keeps all history. It needs one of `title`, `workspace_id`,
`reference_prefix` and `hidden`. Only the owner changes `title`, `workspace_id` and
`reference_prefix`. `rename_workspace` was removed: a write of it is refused with
`rename_workspace was removed; use update_workspace`.
A new `workspace_id` becomes the public ID in lists and reads. Old names stay aliases with
the same access, and no other workspace can take them. A new `reference_prefix` changes the
prefix of new public IDs. Old IDs stay valid.

`hidden: true` hides the workspace only for the member who sends it. A member of any role,
also an agent or a runner, may send it. The workspace leaves the web lists of that member:
`GET /api/workspaces` and the board drop-down. A direct board link `/?workspace=<id>` still
opens it, and the drop-down then shows it for that page only. `GET /api/workspaces?all=true`,
Settings and `GET /api/access` still list it. `joppa_list`, `?all=true` and the server
command line give it `"hidden": true` for that member only. Every MCP read and write works on
it as before. `hidden: false` shows it again. `remove_member` clears the flag of the member.
