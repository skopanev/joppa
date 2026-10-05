# Objects and workspace trees

## Common documents

Every object uses an address such as `domain:billing`, `ac:payment` or `note:source`.
The type prefix prevents collisions between old local identifiers.
Permanent public IDs remain valid.

1. Read the object with `joppa_objects`.
2. Send its current `revision` with `save_object`.
3. Read the returned revision after the write.

Use `revision: 0` to create an object.
The server checks the supplied revision under the workspace write lock.
It creates the next revision and records the actor, time, reason and schema revision.
A stale write fails. Identical retries use the same `request_id` and return the same result.

`data`, `parent` and `links` replace the current document fields.
Read the document before you change them. Keep fields that you do not change.
Comments and attachments remain on the object's address.
They do not replace the document or its test evidence.

All documents have a title and an owner.
AC and Task documents can inherit the REQ owner.
An AC with `decision_required: true` has an independent human owner who supplies its decision.
A Task keeps its assignee separate from its owner.

REQ text changes create a new verification revision and reset all AC checks.
Saving a REQ also creates that revision when only its links change.
AC text changes create a new parent REQ revision.
Task text changes use the existing contract checks.
Process actions keep their original commands and authority rules.
Only test results make an AC green.

## Workspace schema

Call `joppa_tree` to read the workspace schema.
Use `set_tree_schema` to replace it:

```json
{
  "op": "set_tree_schema",
  "revision": 0,
  "reason": "Add release grouping",
  "types": {
    "initiative": {"title": "Initiative", "role": "group", "root": true, "parents": []},
    "requirement": {"title": "Requirement", "role": "requirement", "root": false, "parents": ["initiative"]},
    "ac": {"title": "Acceptance criterion", "role": "ac", "root": false, "parents": ["requirement"]},
    "task": {"title": "Task", "role": "task", "root": false, "parents": ["ac"]},
    "note": {"title": "Note", "role": "document", "root": true, "parents": ["*"]}
  }
}
```

Preserve types and parent rules needed by existing objects.
The server rejects a schema that makes the current tree invalid.
Read the current schema before you replace it.
Agents and the workspace owner can configure the schema.
The separate `/tree?workspace=...` page only reads it.

Use custom types with any supported role. A `task` role supplies claim, release, completion and queue operations.
Rename their display titles and set their allowed parents.
`root: true` permits an object without a parent.
`parents: ["*"]` permits any parent type.
The server rejects parent cycles.

The default schema uses REQ → AC → Task. These parent rules are configuration.
Schemas can omit these types. Task parents can name any configured type, including Task itself.
Objects have one parent or no parent when their type allows roots.
Old Tasks with multiple bindings remain under their REQ with `legacy_acs`.
Their original bindings remain in history. Move active Tasks only to parents permitted by the current schema.
Grouping above REQ can use custom types without Domain or Capability.
Each document revision records its schema revision.
Historical schema reads return one selected revision.

## Links and dependencies

An object can link to many objects of any type in the same workspace.
A Note can link to two Domains. A Domain can link to another Domain.
A link does not change tree parentage.

- `related` records context. Cycles are permitted.
- `depends_on` records a dependency and pins its target revision.
- `blocking: true` requires that dependency before work can start.

A blocking target must be an AC, REQ or Task.
Its required result is a verified AC, accepted REQ or completed active Task.
Blocking cycles include inherited parent dependencies and are rejected.
Dependencies can cross REQ and Domain boundaries inside the workspace.

When a target changes, the system returns `review_required` and the changed revision.
A blocking dependency prevents claims until an agent checks and updates its pin.
An informational dependency shows the review hint and permits claims.
The system preserves earlier test evidence.
It does not copy old evidence into a new verification revision.

## Notes

Create new incoming information as `kind: "note"`:

```json
{
  "op": "save_object",
  "kind": "note",
  "id": "customer-feedback",
  "revision": 0,
  "parent": null,
  "data": {"title": "Customer feedback", "owner": "owner", "body": "Original source text"},
  "links": [{"target": "domain:billing", "relation": "related"}],
  "reason": "Record customer feedback"
}
```

Preserve source text in its original language.
A corrected Note creates a new revision. Earlier text remains readable.
Use the same comment and attachment commands as for other objects.
The legacy `note` operation adds a comment. It does not create a Note document.
Note replaces Intake and Point. The current schema has no separate source types.
The server converts existing sources in one migration transaction before it serves requests.
It preserves source text, authors, review context, comments, attachments and numbered history.
Existing public IDs and source addresses resolve to the corresponding Note.
A converted point links to its REQ with `related`.
Old journal operations remain readable. New source operations and source types are rejected.

Link a REQ to a Note with `depends_on` and the Note revision that informed the wording.
A later Note change then produces a review hint.
The agent reads that Note before it changes the requirement.
There is no separate comment-to-Note promotion process.

## Narrow reads

`joppa_objects` without an address lists root objects.
Supply `parent` to list one object's children.
Supply `parent: "*"` to list all current objects, with pagination.
Use `kind` to filter that list.
Lists contain titles, owners, revisions and readiness hints. They do not contain document bodies or history.

Supply `address` to read one current document, its comments, attachments and relation titles.
Use `history: true` to read revision counts.
Use `revision: N` to read exactly one historical document.
Historical reads omit current status, incoming links, comments and attachments.
Outgoing links identify their metadata as current. Removed targets remain readable and have a historical label.
Pinned dependencies open the saved target revision.
Browser cards use the same lazy read paths.
The custom tree supports filters by type, owner and status. Filters read summaries, not document bodies.
The database indexes workspace, parent, type and incoming links.

`joppa_read` retains its REQ verification interface.
Its `revision` selects the REQ verification revision for AC evidence commands.
Task process commands use the Task object address and its own revision.
`joppa_objects.revision` always selects the addressed document's revision.
Read the current Task before you claim, release, review, retire or complete it.

Old authoring commands remain compatible.
Domain and Consumer edits through those commands do not supply a stale-write token.
Use `save_object` for new document edits.
Future changes to historical document formats need an explicit compatible migration.
The projection rejects a different document with an existing revision number.
The immutable event journal is the source for all projections.

## Independent Tasks

Create Tasks with `save_object` and a type whose role is `task`.
The document needs `title`, `owner`, `contract`, `purpose` and optional `projects`.
Use the returned object address as the process command `id`. Omit `req`.
Use the current Task object `revision` for every process command.
Task status changes append object revisions. Claim is atomic.
Queue previews and `claim_next_task` use the same eligibility rule.
Priorities accept any configured object address and apply to its descendants.

A Task type can configure `parent_gate`: `ready`, `verified` or `completed`.
Omit it when parent evidence is not a claim condition.
The default Task type uses `ready`. Its default AC parent must have a clear assessment.
A Task parent does not need completion unless the schema selects `completed`.
A changed parent document requires Task owner review. Parent Task status changes do not change its contract.
Use `review_task` with a reason to confirm the current parent contract.
Use `save_object` to change the parent when the schema permits it.
Only the Task owner can move it. Completed Tasks retain their original contract and evidence.
Task completion never verifies a specification or an AC.

The server migrates old Tasks to independent process records once.
Their addresses, public IDs, completion results and original revisions remain readable.
The server resolves old `req` process addresses to the same Task record.
Historical REQ task fields remain journal snapshots. Use `joppa_tasks` for the current queue.
