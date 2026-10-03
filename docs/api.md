# Journal API

## Technical writing

PREFER ENGLISH. Write new technical text according to ASD-STE100, Issue 9.
At least 80% of sentences in each text must comply. Russian is permitted under the same rules.
Apply this requirement to REQs, ACs, tasks, comments, decisions, descriptions, reasons, scope, and expected and observed results.
Preserve intake and verbatim user text in their original language. Do not translate existing text or rewrite history and evidence.
New REQ and AC wording still requires owner confirmation.
Read the complete [technical writing rules](writing.md), also returned by `joppa_help` under `writing`.

## Commands

POST /api/workspaces/{workspace}/commands: {"request_id":"unique-id","command":{...}}.
GET /api/workspaces; GET /api/workspaces/{workspace}. Authorization: Bearer <token>.
MCP: POST /mcp over HTTPS; actor comes from OAuth or a private service credential.
Every operation below includes "op" with its name. IDs contain ASCII letters, digits, -, _, . (120 bytes max).
Actor is supplied by authentication, not by command fields. Null means optional; omitted Option fields also work.

```text
workspace: title (server administrative CLI only; rejected by HTTP API and MCP)
rename_workspace: title, reason, reference_prefix:null, workspace_id:null; workspace owner only; preserves history
member: id, role (human|agent|runner); workspace owner only
consumer: id, title, description, owner (human member)
edit_consumer: id, title, description, owner, reason; consumer owner or workspace owner
reorder_tree: kind (domain|capability|requirement), parent (null for domains), expected:[id], ids:[id]; workspace owner only
project: id, repository
domain: id, title, owner (human member; legacy authority accepted)
capability: id, title, domain, owner:null (defaults to domain owner)
confirm_tree: (no fields); workspace owner validates agent draft
edit_domain: id, title, owner (legacy authority accepted), reason; owner, or agent changing only an unconfirmed title
set_domain_archived: id, archived (true|false), reason; workspace owner only; subtree must have no requirements
edit_capability: id, title, owner:null (unchanged), reason; workspace owner, or agent changing an unconfirmed title without changing owner
revise: req, expected_revision (0 for new), capability, consumers:[id,...], title, body, owner, reason,
        acs:[{id,text,depends_on:[],owner:null,before_launch:false}]
link_source: req, revision, id, kind, uri, external_id:null, version, relation (origin|supports|contradicts|decision)
withdraw_source: req, revision, id, reason; source author or REQ owner
confirm: req, revision; REQ owner confirms wording
comment: req, revision, id, text, kind (comment|decision|accept|change|reject|correction),
         reply_to:null, ac:null
withdraw_decision: req, revision, comment, reason; only the decision author
assess: req, revision, ac,
        assessment:{ready,reason,owner:null,decision:null}
task: req, revision, id, title, acs:[id], projects:[id],
      purpose (implementation|manual|decision), contract, assignee:null (optional)
      Every new task needs a nonempty technical contract. Existing assigned
      tasks without contract remain readable for journal compatibility.
claim_task: req, revision, id; atomically assign an open task to the authenticated actor
amend_task: req, revision, id, title, contract, projects:[id], reason; correct an
      unfinished Task in the current revision. REQ owner, assignee, or creator
      of an unclaimed Task may amend it. The AC and completion history stay put.
claim_next_task: project (optional), purpose (optional), priorities (optional
      ordered list of kind:id selectors: domain, capability, requirement, ac,
      task, project, consumer). Under the same workspace lock, select the
      oldest claimable task matching the first priority that has one. If none
      match, select the oldest claimable task overall. project and purpose are
      hard filters. Unknown priority targets are errors. Journal a concrete
      claim_task; the response command contains req, revision and id.
      For a read-only preview use joppa_tasks with view=available, limit=1 and
      the same project, purpose and priorities. It applies the same ordering and
      returns no Task when none is available; a later claim_task may lose a race.
release_task: req, revision, id, reason; assignee or REQ owner returns unfinished work to the queue
complete_task: req, revision, id, evidence (decision comment ID for decision tasks)
review_task: req, revision, id, ac, reason; REQ owner confirms that a retained task still belongs to this AC after a revision
retire_task: req, revision, id, reason; REQ owner marks any current active task as no longer applicable while keeping its history and completion result
review_coverage: req, revision, run, reason; REQ owner acknowledges checks removed or changed since the latest earlier plan
define_verification_scope: req, revision, subject_keys:[component-id,...], scope, reason; REQ owner agrees the complete component set and check boundary before the first plan
plan: req, revision, run, subject:{component:immutable-build-or-config-id},
      scope, discovery_complete,
      checks:[{id:"[AC-id] test name",ac:"AC-id",method:"automated"|"manual",expected:["..."]}]
result: req, revision, run,
        evidence:{check,outcome:"pass"|"fail"|"skip",observed:["..."],note,link:null}
finish: req, revision, run
accept: req, revision, run; REQ owner only, latest completed run must pass every AC
revoke: req, revision, reason

Each check is linked to exactly one AC through the sealed plan. Joppa records the
result timestamp on the immutable event: a fresh pass is green, a pass older
than seven days is shown as stale orange, and a failure remains red. The portal
derives these colors from plan/result evidence; agents cannot set a color
directly.
issue: req, revision, id, ac, comment (existing comment ID); owner confirms defect
resolve: req, revision, id, comment; owner confirms resolution, then rerun checks
describe: target, text (2000 chars max), reason; an explanation on ANY object.
          A domain or capability keeps it in itself (they are not versioned).
          Everything else keeps it as a note beside the object — an AC's own
          description belongs to its revision and is never rewritten in place.
note: target, id, text, reply_to:null; a plain comment on ANY object
withdraw_note: target, id, reason; the author only
attach: target, id, name, content_type, size, hash; the bytes are uploaded first to
        POST /api/workspaces/{workspace}/blobs (raw body, 10 MB max), which answers
        {hash,size}; any number of attachments per object
detach: target, id, reason; the file itself stays — it may be attached elsewhere

target addresses an object: domain:<id>, capability:<id>, consumer:<id>, requirement:<id>,
ac:<req>/<ac>, intake:<id>, point:<intake>/<point>.
joppa_read returns only COUNTS of notes in the index; ask target=<address> for one
object's description, comments and attachments in full.

intake: id, said (verbatim), author (source attribution, not authenticated identity), owner:null (defaults to actor)
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
For new revisions, the first sealed plan is compared with the latest plan from any earlier revision. The card and read summary list checks whose identity, AC, method or expected evidence disappeared or changed. The REQ owner must call `review_coverage` with a reason before accepting that revision. Old journal revisions predate this rule and remain replayable.
Before the first plan on a new revision, its REQ owner must call `define_verification_scope`. A plan's `subject` must contain exactly those component keys, its `scope` must match the agreed boundary, and it must include every project named by active tasks. Build/configuration values cannot be bare environment names such as `staging`. The first plan seals this scope contract; changing it requires another REQ revision. Older journal revisions remain readable under their historical rules. This validates the declared contract, not the semantic completeness of the owner's component list.
Tasks remain in the current card across REQ revisions, including completed tasks and tasks from older revisions that an earlier edit omitted. Each active carried task needs an explicit `review_task` by the REQ owner before unfinished work can be completed or claimed again. The review binds it to exactly one AC in the new revision. If the task no longer applies, the owner uses `retire_task` with a reason; its earlier revisions, completion result, and original AC remain in history. New tasks belong to exactly one AC. `joppa_tasks` exposes paginated `available`, `mine`, `waiting` and `all` queues from the current projection without reading every requirement. `available` uses the same eligibility rules as `claim_task`: one current AC, an explicit contract, no owner-review hold, no prior assignee/completion/retirement, and a ready AC. Decision-purpose tasks can be claimed by the designated human owner before that AC is ready, because making its decision is the work. An unconfirmed REQ can have provisional work, but cannot pass verification. Task status is derived as open, in_progress or done; an unready AC makes an open task ineligible to claim rather than creating a Task-level blocked status. Claim and release do not make an AC green; only checks do.
The queue response uses the public workspace ID shown by `joppa_list`; historical storage aliases remain valid inputs. `created_at` is the Task creation journal position, not a wall-clock timestamp.
The latest **started** run among completed runs is current. A delayed older run cannot replace a newer verdict.
Read responses include the entire journal with authenticated actor and database timestamp.
Corrections use new comments with reply_to; historical snapshots and decisions are never rewritten.

A specialist decision may be reused from an older revision only when the REQ title/body
and that AC's text, owner, dependencies and launch condition are exactly unchanged.
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

The portal is read-only. Browser session cookies cannot submit commands, upload
blobs or reorder the tree. Use MCP for all product changes, including `note`
(comments on any object) and `comment` (revision discussion).

`POST /api/workspaces/{workspace}/order` is a legacy integration endpoint. It accepts
`{"request_id":"unique-id","kind":"domain","parent":null,"expected":["a","b"],"ids":["b","a"]}`.
An authenticated workspace owner, service bearer credential and same-origin Origin
header are required. Browser cookies are insufficient.
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

Copy buttons at all four levels copy only this permanent ID
or AC text. Resolve a copied ID through `joppa_read({"item_id":"..."})`; it returns
the authorized workspace and `selection` with kind and local command ID. AC selections
also include the parent REQ and selected revision (current by default). An AC absent
from that revision has `selection.present=false`; resolution never silently reads an older revision.
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
exchanges. Access tokens last one hour. Refresh tokens last 30 days, rotate
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

Workspace renames may set `workspace_id` to a new unique public ID. Lists and read
responses then return that ID. Previous workspace names remain permanent aliases;
existing item references, journal records and access boundaries are preserved.
Access rules can use either name. Names cannot be claimed by another workspace.

Antigravity CLI uses manual code entry with a hosted callback that only displays
the code. For its exact callback URL, Joppa displays a Copy code button directly
on the consent page. The code remains bound to the registered callback, client
and PKCE verifier, is single-use, and expires after five minutes. Other clients
return to their registered callback automatically.

## Automatic IDs and batches

Through `joppa_command`, omit `id` when creating a domain, capability, consumer, project,
intake, comment, task or issue. Omit `run` for a new check plan. For `revise`,
omit `req` only when `expected_revision` is 0; omit `id` on newly added ACs.
The server assigns UUIDs and returns the resolved `command` and `system_ids`
for domains, capabilities, REQs and ACs. Explicit local IDs remain supported.
An omitted field means a new identity; null or an empty string is not an ID.

When revising, retain the existing REQ and AC IDs. Do not omit IDs on existing
ACs to change their wording. Parent references, member identities, owners,
authorities and assignees must identify existing objects or people. They are
never guessed or automatically created. Archived IDs remain reserved.

A successful request persists its original input and result in an immutable
receipt. Retry exactly the same input with the same `request_id` to get the
same generated IDs. Different input or a different actor is rejected. A failed
transaction consumes neither IDs nor the request ID. Do not change request IDs
merely because a successful response was lost.

`joppa_batch` accepts `workspace`, `request_id` and `operations` (1–100 entries).
Every entry has `command` and an optional unique `ref` label. Commands execute in
order in one database transaction: either all append, or none do. Each operation
retains its own actor/time/history entry. Permissions are identical to individual
commands; a batch does not confer approval authority.

A later command can reference a resolved earlier command field using an explicit
object `{"$ref":"label/field"}`. Nested paths use JSON Pointer syntax after the
label, for example `requirement/acs/0/id`. Forward or missing references fail the
whole batch. Plain text is never interpreted as a template.

```json
{
  "workspace": "product",
  "request_id": "create-access-tree-1",
  "operations": [
    {"ref":"area","command":{"op":"domain","title":"Access","owner":"owner"}},
    {"ref":"ability","command":{"op":"capability","title":"Restore paid access","domain":{"$ref":"area/id"}}},
    {"ref":"buyer","command":{"op":"consumer","title":"Paying customer","description":"Uses paid features","owner":"owner"}},
    {"ref":"requirement","command":{
      "op":"revise","expected_revision":0,"capability":{"$ref":"ability/id"},
      "consumers":[{"$ref":"buyer/id"}],
      "title":"Paid access survives signing in again","body":"Only iOS is in scope",
      "owner":"owner","reason":"Agreed product scope",
      "acs":[{"text":"On iOS, an active subscription unlocks paid features after signing in again"}]
    }}
  ]
}
```

The response has `results` in operation order, each with `position`, resolved
`command` and applicable `system_ids`; `summary` and `replayed` describe the
package. Referenced values and generated IDs remain identical on replay.
The administrative typed command interface still expects explicit local IDs;
interactive agents should use the HTTPS MCP tools.

## Methodology through MCP

Call `joppa_model_docs` before authoring a product tree. Optional `section` is
`model`, `process`, `green`, `writing` or `all` (default). The response contains the English
source documentation compiled into the running server; no local checkout is
needed. `joppa_help` supplies command shapes and this API contract. Initialization
instructions provide the short rules and direct agents to both tools.
The `writing` section supplies the ASD-STE100 rules, language preference, and preservation requirements.
`joppa_help` returns the same section under `writing`. Command field descriptions identify text covered by this requirement.

## Search

`joppa_search` accepts `workspace`, `query`, optional `kind` and `limit` (1–50,
default 10). It returns current matching system IDs with explicit full-text or
hybrid mode and embedding coverage. Workspace authorization applies before any
provider request and before returning results. See [search](search.md) for
freshness, archived data, provider setup and ranking limits.

`joppa_help` also returns the full search guide under `search`, so agents can
read indexing limits and failure behavior without a local checkout.

## MCP revision reads

`joppa_read` returns a workspace index or one requirement's **current revision only**.
Each indexed requirement and selected requirement includes `revision_count`.
A selected requirement also reports `current_revision`; its `revisions` array
contains exactly one revision, and `comments` contains only that revision's discussion.
Its summary describes the selected revision, including when reading an older one.
Object notes are shared across revisions and remain separate from revision discussion.

- `{"workspace":"product","req":"r-xxx"}` reads the current revision.
- `{"workspace":"product","req":"r-xxx","history":true}` returns requirement
  metadata and revision count, with no revision bodies, notes, discussion or event log.
- `{"workspace":"product","req":"r-xxx","revision":2}` reads only revision 2.
- `{"item_id":"<copied REQ or AC ID>","history":true}` and the corresponding
  `revision:2` request follow the same rules. Revision numbers belong to the parent REQ.

No bulk revision or event-log read is exposed by MCP. `history:true` without a
selected requirement returns the index with counts. Explicit `revision` takes
precedence over count-only discovery. Invalid or missing revision numbers produce
an error, never a fallback to current. `cursor` and `limit` from older history
clients are rejected with instructions to select a revision. The portal and
administrative snapshots retain full history.
