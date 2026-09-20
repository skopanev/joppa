# Journal API

POST /api/workspaces/{workspace}/commands: {"request_id":"unique-id","command":{...}}.
GET /api/workspaces; GET /api/workspaces/{workspace}. Authorization: Bearer <token>.
MCP: POST /mcp over HTTPS; actor comes from OAuth or a private service credential.
Every operation below includes "op" with its name. IDs contain ASCII letters, digits, -, _, . (120 bytes max).
Actor is supplied by authentication, not by command fields. Null means optional; omitted Option fields also work.

```text
workspace: title
rename_workspace: title, reason, reference_prefix:null; workspace owner only; preserves workspace ID and history
member: id, role (human|agent|runner); workspace owner only
reorder_tree: kind (domain|capability|requirement), parent (null for domains), expected:[id], ids:[id]; workspace owner only
project: id, repository
domain: id, title, authority (human member)
capability: id, title, domain
confirm_tree: (no fields); workspace owner validates agent draft
edit_domain: id, title, authority, reason; workspace owner only
set_domain_archived: id, archived (true|false), reason; workspace owner only; subtree must have no requirements
edit_capability: id, title, reason; workspace owner only
revise: req, expected_revision (0 for new), capability, title, body, owner, reason,
        acs:[{id,text,depends_on:[],decider:null,before_launch:false}]
confirm: req, revision; REQ owner confirms wording
comment: req, revision, id, text, kind (comment|decision|accept|change|reject|correction),
         reply_to:null, ac:null
withdraw_decision: req, revision, comment, reason; only the decision author
assess: req, revision, ac,
        assessment:{ready,reason,recipient:null,decision:null}
task: req, revision, id, title, acs:[id], projects:[id],
      purpose (implementation|manual|decision), assignee
complete_task: req, revision, id, evidence (decision comment ID for decision tasks)
plan: req, revision, run, subject:{component:immutable-build-or-config-id},
      scope, discovery_complete,
      checks:[{id:"[AC-id] test name",ac:"AC-id",method:"automated"|"manual",expected:["..."]}]
result: req, revision, run,
        evidence:{check,outcome:"pass"|"fail"|"skip",observed:["..."],note,link:null}
finish: req, revision, run
accept: req, revision, run; REQ owner only, latest completed run must pass every AC
revoke: req, revision, reason
issue: req, revision, id, ac, comment (existing comment ID); owner confirms defect
resolve: req, revision, id, comment; owner confirms resolution, then rerun checks
intake: id, said (verbatim), author (source attribution, not authenticated identity)
split: id, points:["verbatim point"]
questions: kind (bug|policy|question|idea), fields:["field-id"]; workspace owner only
sort: id, point, kind
answer: id, point, field, value
route: id, point, outcome (work|refused|elsewhere), reason, req:null
```

A run covers all ACs of one revision on one subject (map of component builds/configurations).
Missing results and skipped checks are n/a. Manual pass requires identical expected/observed sets.
Manual evidence is submitted by a human member; automated evidence and finish by the run reporter.
Expected tests cannot disappear between runs of one revision. To replace the verification contract, save a new revision with a reason.
The latest **started** run among completed runs is current. A delayed older run cannot replace a newer verdict.
Read responses include the entire journal with authenticated actor and database timestamp.
Corrections use new comments with reply_to; historical snapshots and decisions are never rewritten.

A specialist decision may be reused from an older revision only when the REQ title/body
and that AC's text, decider, dependencies and launch condition are exactly unchanged.
The new revision still needs fresh checks and owner confirmation. The assessment cites
the prior decision explicitly. Withdrawal blocks every AC assessment that cites it.

## Browser authentication

Joppa owns its Google OAuth client and invitation list; there is no dependency
on another product's authentication service.

`POST /auth/login` returns a Google authorization URL and sets an HttpOnly attempt
cookie (`SameSite=Lax`, so Google's top-level callback can identify the browser).
`GET /auth/google/callback` validates the state and browser binding, exchanges the
code server-side with PKCE, and verifies Google's signed ID token and Joppa access.
`POST /auth/poll` returns `pending` until completion, then sets a separate 12-hour
HttpOnly `SameSite=Strict` session cookie. Attempts cannot be collected twice.
`POST /auth/logout` invalidates the session server-side.

Browser sign-in, sign-out, polling and OAuth consent require an exact same-origin `Origin` header.
Content command requests without an Authorization header
return 403, including same-origin requests with a valid session cookie. Cookies are `Secure` over HTTPS. Session token hashes and revocations persist in PostgreSQL; sign-in survives
server restarts until the original 12-hour expiry or explicit sign-out. `GET /api/me` returns the authenticated Joppa actor and its verified Google email
(or null for an identity without a Google binding).
Bearer tokens remain available for service integrations. The browser never
stores or displays them. Missing Google configuration does not enable a fallback.

## Tree ordering and item history

`POST /api/workspaces/{workspace}/order` is the only browser write. It accepts
`{"request_id":"unique-id","kind":"domain","parent":null,"expected":["a","b"],"ids":["b","a"]}`.
An authenticated workspace owner and same-origin Origin header are required.
Unknown fields are rejected. `expected` must match the current sibling order;
`ids` must contain exactly those siblings once. Moves between parents are rejected.
Retrying the same request ID is idempotent. New siblings append to the saved order.
The same operation is available through MCP as `reorder_tree`.

Every completed operation appends one journal event with actor and database time.
Each domain, capability and requirement exposes its relevant events in History.
REQ wording and AC changes are saved together through `revise`, never per keystroke.
Reordering records an event but leaves revisions, acceptance and check evidence intact.

## Permanent item IDs

Read responses include `system_ids`, grouped by `domain`, `capability`, `requirement`
and `ac`. Each ID is `workspace-D|C|R|A-XXXXXXXXXX` with a random 10-character
uppercase alphanumeric suffix and a database-wide unique constraint. A collision
is retried; IDs are immutable and survive wording changes, revisions and ordering.
Existing cards receive IDs additively without rewriting journal events.

Copy buttons at all four levels copy only this permanent ID and the selected title
or AC text. Resolve a copied ID through `joppa_read({"item_id":"..."})`; it returns
the authorized workspace and `selection` with kind and local command ID. AC selections
also include the parent REQ and latest revision containing the AC.
`joppa_read({"workspace":"..."})` remains available; supply exactly one selector.
Commands continue to use local IDs from the resolved workspace, never guessed slugs.

An explicit `rename_workspace` with `reference_prefix` changes the prefix used by
new copies and newly created items. It appends aliases and preserves all old IDs,
content, history and workspace permissions. Title-only renames leave IDs unchanged.


## HTTPS MCP authentication

Clients connect directly to `/mcp` over Streamable HTTP. A missing or invalid
credential returns 401 with a `WWW-Authenticate` resource-metadata URL.
`/.well-known/oauth-protected-resource/mcp` names the authorization server;
`/.well-known/oauth-authorization-server` advertises its endpoints.

`POST /oauth/register` registers public clients with exact HTTPS or loopback
redirect URIs. `/oauth/authorize` requires code and PKCE S256. The consent page
uses Joppa's existing Google browser sign-in. `/oauth/approve` requires a browser
session, exact same Origin and explicit consent, then returns a single-use code.
`POST /oauth/token` accepts form-encoded authorization_code or refresh_token
exchanges. Access tokens last one hour. Refresh tokens last 90 days, rotate
on every use, and a replay revokes the entire token family.
`POST /oauth/revoke` with token and client_id revokes that family.

The only resource is the canonical `/mcp` URL. An explicitly different resource
is rejected; older clients that omit it receive a token for `/mcp` only.
MCP tokens cannot be used as browser cookies or REST API credentials.
Notifications return 202 without a body. GET returns 405 after authentication;
this server does not require an SSE stream or protocol session.

The former local client, stdio transport, device-code login and `/connect`
page have been removed. Existing device-key hashes remain historical records
in the WORM database, but are no longer accepted for authentication.

Archived domains and their capabilities remain in read responses for history and
ID resolution, but are absent from the active tree. New capabilities and REQs cannot
be created beneath them. `set_domain_archived` with `archived:false` restores them.
IDs are never recycled. Domains with requirements cannot be archived.
