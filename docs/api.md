# Common object API

Read [Objects and workspace trees](objects.md) before new document edits.
Use `joppa_objects`, `joppa_tree`, `save_object` and `set_tree_schema`.
Pass the current `revision` with each document or schema write.
Current criterion process metadata includes `assessment.state`: `unassessed`, `waiting`, or `ready`.
It also includes the current specification revision, reason, recipient, and decision reference.
Task process and queue metadata include the direct parent assessment when that parent has the configured criterion role.
`readiness_blocker` identifies the assessment that prevents a configured readiness gate, including criterion dependencies.
A waiting Task reports the same question and recipient in `waiting_reason` and its claim refusal.
Decision Tasks retain the configured exception for collecting their parent's decision.
These fields describe readiness. They do not change check evidence or work completion.
The following operations retain their original verification and compatibility rules.

`archive_object` requires `target`, `revision` and `reason`. It archives an object with no live children whose configured role is `group` or `document`.
`restore_object` uses the same fields. It restores an object archived with `archive_object` if the current schema permits its parent.
Both commands require an agent or the workspace owner. They append a revision and preserve IDs, links, fields and annotations.
Archive a group only after moving or archiving its active children. Restore its parent before restoring a child.

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
delete_workspace: reason; workspace owner only; removes every row of the workspace. Before the purge, each workspace whose remote links or child workspace bindings name it gets one internal remove_workspace_references event that removes them. The result lists them in removed_references. The owner reads that list first with GET /api/workspaces/{workspace}/references.
member: id, role (human); workspace owner only. An agent enrolls through its agent token, not through member. An older journal event keeps its agent or runner role in a replay.
consumer: id, title, description, owner (human member)
edit_consumer: id, title, description, owner, reason; consumer owner or workspace owner
reorder_tree: kind (any configured type), parent (complete object address or null), expected:[id], ids:[id]; workspace owner only
project: id, repository
domain: id, title, owner (human member; legacy authority accepted)
capability: id, title, domain, owner:null (defaults to domain owner)
confirm_tree: (no fields); workspace owner validates agent draft
edit_domain: id, title, owner (legacy authority accepted), reason; owner, or agent changing only an unconfirmed title
set_domain_archived: id, archived (true|false), reason; workspace owner only; subtree must have no requirements
edit_capability: id, title, owner:null (unchanged), reason; workspace owner, or agent changing an unconfirmed title without changing owner
revise: req, expected_revision (0 for new), capability, consumers:[id,...], title, body, owner, reason,
        acs:[{id,text,depends_on:[],owner:null,before_launch:false}]
        A new criterion takes the first valid consumer of `consumers` as its single `consumer`.
link_source: req, revision, id, kind, uri (optional), external_id:null, version, relation (origin|supports|contradicts|decision)
      Supply a nonempty uri or external_id. Omit uri when the source exists only
      in an external tool. The journal and reads store an omitted URI as an empty
      string. When both references exist, the source retains both.
withdraw_source: req, revision, id, reason; source author or REQ owner
confirm: req, revision; REQ owner confirms wording
comment: req, revision, id, text, kind (comment|decision|accept|change|reject|correction),
         reply_to:null, ac:null
withdraw_decision: req, revision, comment, reason; only the decision author
assess: req, revision, ac,
        assessment:{ready,reason,owner:null,decision:null}
task (legacy creation): req, revision, id, title, acs:[id], projects:[id],
      purpose (implementation|manual|decision), contract, assignee:null (optional)
      Every new task needs a nonempty technical contract. Existing assigned
      tasks without contract remain readable for journal compatibility.
      Prefer save_object with a type whose role is task and a schema-permitted parent.
      Process commands below use the full Task address and its own revision.
      Old req arguments remain compatible and use the REQ revision.
claim_task: revision, id; atomically assign an open Task to the authenticated actor
      Claims persist until an explicit recorded action changes the holder or work state.
claim_next_task: project (optional), purpose (optional), priorities (optional
      ordered list of kind:id selectors: domain, capability, requirement, ac,
      task, project, consumer or any configured type). Use the returned object address.
      Old task:<REQ>/<Task> addresses remain valid. A short selector can match
      several Tasks. Use the full address to select one Task.
      Under the same workspace lock, select the
      oldest claimable task matching the first priority that has one. If none
      match, select the oldest claimable task overall. project and purpose are
      hard filters. Unknown priority targets are errors. Journal a concrete
      claim_task; the response command contains the Task address and its object revision.
      For a read-only preview use joppa_tasks with view=available, limit=1 and
      the same project, purpose and priorities. It applies the same ordering and
      returns no Task when none is available; a later claim_task may lose a race.
release_task: revision, id, reason; the holder or workspace owner returns unfinished work to the queue
complete_task: revision, id, evidence; only the current holder completes claimed work
      A decision Task under the default AC gate needs the actual specialist decision comment ID.
review_task: revision, id, reason; Task owner or workspace owner confirms the current parent contract
retire_task: revision, id, reason; Task owner or workspace owner retires work and retains its history and completion result
      The planning holder of the parent criterion can also retire a blocked child Task.
      This authority applies only to blocked Tasks.
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

Each check links to one criterion through the sealed plan. Joppa records its
result timestamp on the immutable event. Elapsed time alone does not expire a
pass or change its color. A pass applies to its recorded context revision and
subject. Another revision or subject needs another run. Include all relevant
code, test and environment versions in `verification_scope.subject_keys`.
The launch caller must supply the requested versions. The portal derives
colors from the recorded verdict. An integration cannot set a color directly.
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
ac:<req>/<ac>, note:<id>, task:<req>/<task>, project:<id>.
joppa_read returns only COUNTS of notes in the index; ask target=<address> for one
object's description, comments and attachments in full.

```

A run covers all ACs of one revision on one subject (map of component builds/configurations).
Missing results and skipped checks are n/a. Manual pass requires identical expected/observed sets.
Manual evidence is submitted by a human member; automated evidence and finish by the run reporter.
Expected tests cannot disappear between runs of one revision. To replace the verification contract, save a new revision with a reason.
For new revisions, the first sealed plan is compared with the latest plan from any earlier revision. The card and read summary list checks whose identity, AC, method or expected evidence disappeared or changed. The REQ owner must call `review_coverage` with a reason before accepting that revision. Old journal revisions predate this rule and remain replayable.
Before the first plan on a new revision, its REQ owner must call `define_verification_scope`. A plan's `subject` must contain exactly those component keys, its `scope` must match the agreed boundary, and it must include every project named by active tasks. Build/configuration values cannot be bare environment names such as `staging`. The first plan seals this scope contract; changing it requires another REQ revision. Older journal revisions remain readable under their historical rules. This validates the declared contract, not the semantic completeness of the owner's component list.
Tasks are independent objects. Their parent follows the workspace schema. A Task can also be a root when its type permits it.
The default tree places Tasks under AC with `parent_gate: "ready"`. Other schemas can select different parents, nesting and gates.
Tasks retain their history and completion results when a parent changes. A changed parent contract requires `review_task` before claim or completion.
The Task owner or workspace owner confirms that contract. Use `save_object` to move a Task to another permitted parent.
Completed Tasks retain their original contract. Use `retire_task` when work no longer applies.

`joppa_tasks` exposes paginated `available`, `mine`, `waiting` and `all` queues from the current projection.

`joppa_objects` with `work_filter="unstarted"` lists current criteria that need planned work.
It selects criterion and Task roles from the workspace schema, regardless of their type names.
Candidates need a ready assessment, current dependencies, and verification other than `pass`.
Missing assessments and waiting decisions do not qualify. Current object metadata shows their reasons and recipients.
An active Task binds to its nearest criterion through tree parents, including intermediate groups.
This binding stops at the nearest specification. A Task attached to a specification does not claim its individual criteria.
Completed and retired Tasks remain readable and do not count as active work.
Each candidate includes `work_summary` counts for active, done and retired Tasks.
A candidate with done Tasks can need verification instead of new implementation. The list does not create Tasks. Check `work_summary.done` before planning new work.
The filter lists matching criteria across the tree by default. An explicit `parent` still selects direct children.
The existing cursor, kind and related-object filters remain available. A workspace change invalidates the cursor.
Candidates include current object revisions and assessment source revisions. Lists do not load revision bodies or the journal.
`available` uses the same eligibility rules as `claim_task`: an explicit contract, reviewed parent context, satisfied dependencies and the configured parent gate.
The Task must have no active claim, completion or retirement.
Elapsed time, missing heartbeats and server restarts never return a Task to the queue.
Only the current holder can complete claimed work.
The holder or workspace owner can release unfinished work with a reason.
Earlier journal entries and claim metadata remain readable.
Queue cursors retain their read time until the workspace position changes. A decision Task requires its designated human owner to claim it.
Under the default AC gate, that owner can claim decision work before the AC becomes ready.
Task status is `open`, `in_progress` or `done`. Unmet dependencies prevent claim and appear in readiness hints.
Task completion does not verify an AC or specification. Only verification evidence can make an AC pass.
The queue response uses the public workspace ID shown by `joppa_list`; historical storage aliases remain valid inputs. `created_at` is the Task creation journal position, not a wall-clock timestamp.
The latest **started** run among completed runs is current. A delayed older run cannot replace a newer verdict.
Read responses include the entire journal with authenticated actor and database timestamp.
Corrections use new comments with reply_to; historical snapshots and decisions are never rewritten.

A specialist decision may be reused from an older revision only when the REQ title/body
and that AC's text, owner, dependencies and launch condition are exactly unchanged.
The new revision still needs fresh checks and owner confirmation. The assessment cites
the prior decision explicitly. Withdrawal blocks every AC assessment that cites it.

## Note processing

`set_note_status`: `target`, `revision`, `status` (`unprocessed` or `processed`), `reason`.
Read the Note revision first. The command preserves its source data, parent and links.
The server appends a document revision. Source changes return the Note to `unprocessed`.
`joppa_objects` supports `note_status` for any type with `inbox: true`, and `related_to` for links in either direction.
A list read takes `fields`, a list of key names. Each item then keeps only these keys and its `address`. The envelope and `next_cursor` stay. Over HTTP, send `fields=title,signal`.
`joppa_tests` without `ac` and `task` gives the overview of green and red criteria, the same as `GET /tests`. With `summary=true`, each criterion gives `counts` per test status in place of `tests`.

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
Note, comment, task or issue. Omit `run` for a new check plan. For `revise`,
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

### Task object commands

Create a Task with `save_object` under any parent permitted by the workspace schema.
Its type must have role `task`. The default type name is `task`.
Use `id: "task:<local-id>"` or the returned custom type address for process commands.
Omit `req`. Supply the current Task object `revision`.
`claim_task`, `amend_task`, `release_task`, `complete_task`, `review_task` and `retire_task` share this address.
Generic `review_task` takes `reason` and does not take `ac`.
Queue items report their object `address`, `parent`, `revision` and current status.
Historical `req` addresses remain accepted and resolve to this same process record.
The Task type configures parent evidence through optional `parent_gate`.
See `docs/objects.md` for the gate values and migration contract.

## Configured verification types

Read the workspace schema before you create specification or criterion documents.
Use `save_object` with the configured type name.
A type with role `requirement` supports the verification workflow.
A type with role `ac` supplies a criterion in its nearest ancestor specification.
Use the returned `process.req`, `process.criterion` and verification revision in process commands.
For custom types, these process keys are full addresses such as `spec:billing` and `measure:paid-access`.
Use the same criterion key in `plan.checks[].ac` and in the automated test tag, such as `[measure:paid-access]`.
Legacy process keys remain compatible.
Public copied IDs are opaque. Read their type from the object response.


## Task check evidence

A sealed check can include `tasks: [{"address": "task:ID", "origin_revision": 2}]`.
Read `process.origin_revision` from the Task. This identifies its stable contract.
Claim, release and completion change the object revision but preserve that contract revision.
The server rejects stale, absent, retired, foreign-context or duplicate bindings atomically.
Task names and permitted parents follow the workspace schema.
A criterion Task must match the check criterion. A specification Task can use any check within that context.

Use the existing `plan`, `result` and `finish` operations. Seal the plan before execution.
The latest started run decides Task verification. A newer unfinished run makes it unverified.
All declared Task checks must pass. A failed check gives fail. Missing, skipped or absent checks give unverified.
Task verification does not complete work or confirm its specification.
A Task without declared bindings cannot inherit a parent pass.
Changed contracts, source context and dependencies invalidate current evidence without deleting history.
Required bindings cannot disappear from a later plan. Changed coverage needs the existing owner review.

The existing test adapter preserves these bindings. It can use the authenticated MCP client:

```sh
python3 scripts/run_checks.py WORKSPACE checks.json --joppa ./client/joppa
```

Use the existing `JOPPA_MCP_URL` and private `JOPPA_TOKEN_FILE` settings.
The adapter reports actual command outcomes. It seals the plan before it starts a command.
It reports each failure and then finishes the run. No extra test runner is required.

`GET /api/workspaces/:workspace/tasks` reads the same queue as `joppa_tasks`.
Use view, limit, cursor, project, purpose and comma-separated priorities as query parameters.

## Workspace timeline

`GET /api/workspaces/:workspace/timeline` and `joppa_timeline` return the same page.
The timeline has one entry for each journal event, in journal order.
Each entry has `position`, `at`, `actor`, `actor_type`, `address`, `title`, `revision`, `change`, `op` and `reason`.
`revision` is the object revision at that change. The board opens it with `?object=ADDRESS&revision=N`.
`change` is save, move, archive, restore, comment, file, tests, readiness, priority, task, results, planning, member, schema, workspace or other.
A move is a save that changes only the parent.
A record of test results appends a `record_report` event, so it has a journal position too. Its change is `results` and its address is `record/ID`.
Use `after=POSITION` to read only later entries. Use `limit` (1 to 100, default 50).
Send `next_cursor` as `cursor` to read the next page. The last page has `next_cursor: null`.
`order=newest` reads the newest entries first; its cursor reads older entries.
Members and agents of the workspace read the timeline. Another account gets 404 `Workspace not found`.
The board shows the timeline at `/?workspace=W&view=timeline`, newest first.


## Planning a criterion

Planning operates on the schema criterion role. The type name can differ from `ac`.

- `claim_planning {ac, revision}` claims a ready, unverified criterion at its current object revision.
- `finish_planning {ac, reason}` publishes the Task set and removes the planning claim.
- `release_planning {ac, reason}` removes the claim and retires only its new Tasks.

The authenticated member supplies the command actor. Each workspace permits one planning claim per criterion and per actor.
Existing Tasks do not prevent a planning claim. Claims remain until an explicit finish or release.
Current criterion reads expose `process.planning_claim`: `null` or `{actor, position}`. Position identifies the claim journal event.

New Tasks have the planning gate only when their creator matches the holder and their creation position follows the claim.
They remain visible in `all`, with `waiting_reason="Task is awaiting planning publication"`. They cannot enter `available` or receive a work claim.
Existing Tasks keep their ordinary gates and active worker assignments. Claimed criteria stay outside `work_filter=unstarted`.

Finish requires a Task creation or revision by the holder after the claim. It validates dependency targets and reachable dependency cycles across the workspace.
Finish also checks each blocked, unretired child Task. It compares the current document with the document revision at the block event position.
The compared contract contains the title, contract, projects and blocking dependency links. The order of links does not change the result.
An unchanged blocked child Task makes finish fail with "blocked Task needs a new contract or retirement".
This refusal takes precedence over "planning produced no Task changes". The failed finish keeps the planning claim and every block.
A successful finish clears `process.blocked` for each changed blocked child Task in the same finish event.
Finish removes only the planning gate and these blocks. It does not change verification or ordinary dependency gates.
Only the holder can finish. The holder or workspace owner can release with a non-empty reason.
Release preserves existing Tasks, their current documents, worker assignments, and history. Retired new Tasks retain their documents and history.
