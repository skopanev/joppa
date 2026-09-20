# Journal API

POST /api/workspaces/{workspace}/commands: {"request_id":"unique-id","command":{...}}.
GET /api/workspaces; GET /api/workspaces/{workspace}. Authorization: Bearer <token>.
CLI: JOPPA_ACTOR=alice joppa command product request-id < command.json.
Every operation below includes "op" with its name. IDs contain ASCII letters, digits, -, _, . (120 bytes max).
Actor is supplied by authentication, not by command fields. Null means optional; omitted Option fields also work.

```text
workspace: title
rename_workspace: title, reason; workspace owner only; preserves workspace ID and history
member: id, role (human|agent|runner); workspace owner only
project: id, repository
domain: id, title, authority (human member)
capability: id, title, domain
confirm_tree: (no fields); workspace owner validates agent draft
edit_domain: id, title, authority, reason; workspace owner only
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

All POST auth endpoints require an exact same-origin `Origin` header.
Browser sessions are read-only: command requests without an Authorization header
return 403, including same-origin requests with a valid session cookie. Cookies are `Secure` over HTTPS. Sessions do not
survive a server restart. `GET /api/me` returns the authenticated Joppa actor and its verified Google email
(or null for an identity without a Google binding).
Bearer tokens remain available for CLI/agent integrations. The browser never
stores or displays them. Missing Google configuration does not enable a fallback.
