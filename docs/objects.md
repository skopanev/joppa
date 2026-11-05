# Objects and workspace trees

## Common documents

Every object has an address such as `domain:billing`, `ac:payment` or `note:source`.
The type prefix keeps the local IDs of two types apart. Permanent public IDs stay valid.

1. Read the object with `joppa_objects`.
2. Send its current `revision` with `save_object`.
3. Read the returned revision after the write.

Use `revision: 0` and omit `id` to create an object. The server makes the ID.
The server checks the revision under the workspace write lock. A stale write fails.
Each write adds a revision with the actor, time, reason and schema revision.
Retry identical input with the same `request_id` to get the same result.

`data` and `parent` replace the current document fields. Sent `links` replace the current
links. A save without `links` keeps them, and `links: []` removes every link.
Read the document before you change them. Keep the fields that you do not change.
A link to an archived or absent object is refused, unless the current revision already has it.
Ordinary document fields can hold text, numbers, lists or maps.

All documents have a title and an owner. The text of every type is `data.body`: the rule of
a REQ, the observable condition of an AC (`Given / When / Then` for a flow, or a direct rule for
an invariant, limit or disclosure), the technical contract of a TAC (`Technical Criterion`,
placed under an AC or a REQ) and the contract of a Task. A save with `data.description`
or `data.contract` is refused: `use body instead of description; read joppa_help`.
An AC, a TAC and a Task can inherit the REQ owner. A Task keeps its assignee separate from
its owner. An open question on a node is its readiness mark: `assess` with `ready: false` and
the question as `reason`. A save with `data.decision_required` is refused.

A save of a criterion (`ac` or `tac`) gives its specification a new revision with the reason
`Criterion <address> changed: <reason>`. A save of a criterion with another parent moves it.
Process commands keep the document fields. Only test results make a criterion green.

Each type can define a `question`. Read it from `joppa_tree`, or from `joppa_help` with
`workspace` and `section: "tree"`. The title and body of an object answer the question of its
type. A save does not send the question, and the server does not check the answer.

The workspace schema gives the types, parents, required fields, status rules and test rules.
See [the workspace schema](schema.md).

## Links and comments, not object text

Keep each object text to its own contract. Do not add other data to it.

1. Link objects with links, never with text.
   - Send the `links` array of `save_object`: `[{"target": "task:<id>", "relation": "related"}]`. Use `depends_on` for a dependency.
   - Use the reference fields of the schema, for example `consumer` or `projects`.
   - Use `set_remote_links` for an object of another workspace.
   - Do not paste lists of Tasks, tickets or addresses into `body`.
2. Add findings, progress and supplements as comments, never as object text.
   - Send `{"op": "note", "target": "<kind>:<id>", "id": "<unique id>", "text": "..."}`.
   - A comment keeps its author and time. It adds no object revision and does not reset tests.
   - The field limit of a type (`field_max_length`, default 1024 characters) does not apply to a comment. Over 1 kb: prefer attach as a file.
   - Do not rewrite or extend `body` with these texts.

One `joppa_batch` can save an object and add its comments atomically.
In text, write a reference as [Title](kind:id). A Markdown link does not replace a structural link.
See [links and other workspaces](links.md) for relations, dependencies and remote links.

## Notes

Save new incoming information as `kind: "note"` with `parent: null`. Link it with `related`
to each object that it informs. The server refuses a Note with a parent.

```json
{"op": "save_object", "kind": "note", "revision": 0, "parent": null,
 "data": {"title": "Customer feedback", "owner": "owner", "body": "Original source text"},
 "links": [{"target": "domain:billing", "relation": "related"}], "reason": "Record customer feedback"}
```

Keep the source text in its original language. A corrected Note gets a new revision.
The `note` operation adds a comment. It does not create a Note document.
Link a REQ to a Note with `depends_on` and the Note revision. A later Note change then asks
for a review of the requirement.

A type with `inbox: true` holds incoming information. A missing `data.status` means
`unprocessed`. When the agent has processed the Note, archive it with `archive_object`.

## Tests of a criterion

A test is a node of a type with role `test` under its criterion (`ac` or `tac`). Save one node
for each test with `save_object`:
`{"kind": "test", "parent": "ac:x", "data": {"title": "<test name>", "owner": "..."}}` (or
`"parent": "tac:x"`).
No criterion, no test: every test in the repository has a present test node, and a full-run record of
`joppa_record_results` gives `"unbound": []` before a merge. See [what green means](green.md)
for the tones and the rollup.

## Tasks

Create a Task with `save_object` and a type whose role is `task`. The `body` is the contract.
A Task has no purpose: `save_object` refuses `data.purpose` with
`a Task has no purpose; read joppa_help`. A member who can write can change any Task.

- `data.assignee` names the member ID that does the work.
- `data.commit` gives the Git sha of the finished work.
- `assess` sets the readiness of the Task. `archive_object` ends it.
- Read Tasks with the node filters of `joppa_objects`, for example `{"kind":"task","assignee":"alex","ready":true}`.

## Archive and restore

`archive_object {target, revision, reason}` archives an object that has no live children.
`restore_object` takes the same fields and needs a parent that the schema permits.
Both keep IDs, links, fields and comments. IDs are never used again.

## Reads

`joppa_objects` without `address` lists root objects. `parent` lists the children of one
object, and `parent: "*"` lists all current objects in pages. `kind`, `data`, `related_to`
and `note_status` filter the list. Lists give no bodies and no history.

`address` gives one current object: `revision`, `parent`, `data`, `comments` and the titled
outgoing `links`. `revision: N` reads one saved revision. `history: true` gives the revision
headers. Resolve a copied public ID with `joppa_search`: the reply gives that object with
`exact_id: true`. See [read contract](read-contract.md) and [read detail](read-detail.md).

Open an object in the board with `/?workspace=WORKSPACE&object=ADDRESS`. Add `revision=N`
to read one saved revision.
