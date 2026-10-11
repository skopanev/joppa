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
Ordinary document fields can contain additional text, numbers, lists or maps after the schema migration.
Process commands preserve those fields. A document replacement removes additional fields that you omit.
Process evidence stays separate. Use process commands to claim work, record results or accept a requirement.

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
A `requirement` role supplies specification revisions, test plans and acceptance.
An `ac` role supplies a criterion in its nearest ancestor specification.
These roles work with any configured type name, including `spec`, `epic`, `measure` and `rule`.
A type name does not select its workflow. Former default names can also use ordinary document or group roles.
All ordinary fields follow the same document rules. Reference fields come only from the active schema.
Custom workflow commands use full object addresses in `req` and `ac`.
The server keeps earlier local journal keys compatible.
Two types can use the same local identifier without sharing test evidence.
A criterion can have intermediate grouping objects above it.
A parent change cannot move existing criterion evidence to another specification.

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

### Required document fields

Set `required_fields` on any configured type:

```json
{"required_fields": {"source": "a signal needs a source"}}
```

Each entry maps a `data` field to its refusal message.
Absent fields, nulls, blank text, empty lists and empty maps have no value.
The numbers and booleans `0` and `false` are valid values.
The default map is empty. The type name does not select this rule.
Rules apply to creation and each later document revision, including legacy commands.
The server rejects a schema if a present object lacks a required value.
Archived and deleted objects do not block the rule.
Supply missing values before you enable the rule.
HTTP returns 422 for an absent required value. MCP returns the configured error.
Failed writes preserve the journal, revisions and request IDs.
Historical reads retain the original document and schema revision.

## Links and dependencies

An object can link to many objects of any type in the same workspace.
A Note can link to two Domains. A Domain can link to another Domain.
A link does not change tree parentage.

- `related` records context. Cycles are permitted.
- `depends_on` records a dependency and pins its target revision.
- `blocking: true` requires that dependency before work can start.

A blocking target can have any configured type.
It needs completion evidence from its semantic role: a verified criterion, a requirement whose current checks pass, or a completed active Task.
Document text and input processing do not supply verification evidence.
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

### Context for external planners

Use object addresses as canonical identifiers for a configured tree.
The `id` field of a common object is its local identifier. Its `address` includes the type prefix.
A verification response can retain a legacy identifier or use a complete object address in `requirement.id`.
Do not compare that field directly with a short input identifier.
Resolve a public identifier with `joppa_read` using `item_id` alone.
Read its `object.address`, then use `joppa_objects` for the current document.

Read a selected object's ancestors through `document.parent`, one object at a time.
Stop when the parent is null. Use the workspace schema to identify semantic roles.
Do not require Domain or Capability names, a fixed depth, or grouping confirmation fields.
Read configured references from the common document and its `references` metadata.
Legacy verification fields such as `capability` and `consumers` do not replace the common document.

Record each source address and its document revision in the implementation plan.
Read linked documents at their pinned revision when the link supplies one.
Before publishing work, compare the relevant current revisions with the recorded source revisions.
Unrelated workspace events do not invalidate an unchanged source contract.
The response `position` identifies the observed workspace state. It does not replace source revision checks.
Reject unknown, inaccessible or cyclic parent context with a clear error.
Never treat missing context as successful discovery.

### Compatibility commands

Old authoring commands remain compatible.
Domain and Consumer edits through those commands do not supply a stale-write token.
Use `save_object` for new document edits.
Future changes to historical document formats need an explicit compatible migration.
The projection rejects a different document with an existing revision number.
The immutable event journal is the source for all projections.

## Independent Tasks

Create Tasks with `save_object` and a type whose role is `task`.
The document needs `title`, `owner`, `contract` and `purpose`.
Its reference fields follow the workspace schema.
Use the returned object address as the process command `id`. Omit `req`.
Use the current Task object `revision` for every process command.
Task status changes append object revisions. Claim is atomic.
Queue previews and `claim_next_task` use the same eligibility rule.
Priorities accept any configured object address.
They match descendants and configured references on a Task or its ancestors.

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

## Note processing and type presentation

Set `inbox: true` on any schema type to hold incoming information.
An omitted flag disables the queue. The type name does not supply a default. Missing `data.status` means `unprocessed`.
Use `set_note_status` with `target`, the object `revision`, `status` and `reason`.
The status is `unprocessed` or `processed`. The server creates the next object revision.
This command preserves the complete source data, parent and links.
A change to source fields reopens a processed input, even when a client sends `processed`.
Owner changes and link changes do not reopen it. Comments and files remain separate annotations.
Use `save_object` for a complete document replacement. Read and preserve its existing fields first.

Each input collection shows unprocessed objects by default. Select All to include processed Notes.
Related notes appear under linked objects. Links can originate from either object.
Processing does not create links or imply that a requirement passes verification.
Use a reason to record a review that needs no product change.

Use `joppa_objects` with the configured input `kind`, `parent: "*"` and `note_status: "unprocessed"` to read incoming work.
Omit `note_status` to read all input objects. Use `related_to: "kind:id"` to read objects linked to that object.
Lists return `note_status` without source bodies or revision history.

Each schema type can set `color: "#RRGGBB"`. Omit it to use the default presentation.
Colors apply to the board and schema diagram. Status circles retain their verification colors.
Each type can set `title_size`, an integer from 12 to 48 pixels. Omit it to use 15 pixels.
The board and tree diagram use this size at every nesting level and screen width.
For example, set 17 for a root type, 15 for its child type, and 14 for deeper types.
Each type can set `badge`, a label of 1 to 8 characters without spaces. Omit it to use initials from its title.
Each type can set `title_preview_length`, from 1 to 256 characters. Omit it to use 64 characters.
This value is also the maximum length of a new title. The server refuses a longer title, so put the context in `body` or `description`.
Older titles can be longer. The board shows a preview that ends with an ellipsis, and the complete title stays in the Details section.
Read the current schema before `set_tree_schema`. Preserve the other type fields.
Presentation changes create a schema revision. They do not change object revisions, parents, or verification evidence.

## Type order

Each type can set `index`, a nonnegative integer. Smaller values appear first.
Root tabs and the schema diagram read this value from the workspace schema.
Types without an index appear after types with an index.
Root tabs use the type ID to order equal or missing values.
The diagram keeps its role order when index values match.
Set the value with `set_tree_schema`. Send the schema revision you read.
Changing an index creates a schema revision. It does not change object revisions or the board layout.
The default recipe places Domain before Consumer, Note and Project.

## Configured references

Each type can define `references`, a map from document fields to reference rules.
The diagram, validation, related-object index and queue priorities use these rules.
Type names do not create reference rules automatically.
The default recipe defines `requirement.consumers` and `task.projects`. Each workspace can replace or remove them.

```json
{
  "title": "Work",
  "role": "task",
  "root": false,
  "parents": ["spec"],
  "references": {
    "affected": {
      "target": "spec",
      "title": "Affects",
      "required": true,
      "multiple": true,
      "verification_subject": false
    }
  }
}
```

`target` names a configured type. Use `"*"` to accept any configured type.
`multiple: true` accepts an array of addresses. Otherwise, supply one address.
`required: true` rejects a missing or empty reference.
Duplicates and missing targets fail validation.
A fixed target permits short local IDs. A wildcard target requires full addresses.
References do not change parentage or create blocking dependencies.
Removing a rule removes its diagram edge and queue meaning. Earlier document revisions remain unchanged.

`verification_subject: true` includes that reference in the Task verification scope check and subject filter.
The scope remains the contract of an ancestor with verification evidence.
Without such an ancestor, the reference records context and queue selection.
A subject filter accepts a full object address. A unique short ID remains compatible.

`parent_decision: true` requires a decision comment for the configured Task parent when completing a decision Task.
The parent must supply verification decision evidence. A free-text result cannot substitute for that evidence.
Omit this option when parent decisions do not form part of the workflow.
The default recipe enables it for Tasks under acceptance criteria.

Existing workspaces receive these settings once through a server migration.
That migration creates a schema revision. It does not rewrite object history.
Type titles and colors remain workspace settings. For example, the `note` type can display the title `Input`.

Set `launch_blocker: true` on a type to use its `before_launch` entries as launch conditions.
Current check evidence and owner acceptance must cover each selected entry.
Use `launch_blocker: false` to disable this behavior. An omitted flag also disables it.
The internal document migration records previous flags once. It does not change any document revision.

A task purpose can be any nonempty label. The `decision` label retains its configured decision behavior.

## Work and verification status

Work status and verification status are separate. Completed work remains `done` when its evidence becomes invalid.
Verification uses `pass`, `fail` and `unverified`. A completed Task passes only when its current parent evidence passes.
A blocking dependency on a requirement waits for its current checks to pass. Owner acceptance remains separate.
If that requirement loses its passing checks, dependent Tasks cannot start. Completed dependent Tasks show `unverified`.
The system preserves completion results and earlier evidence. Ordinary `related` links do not block work.

## Current read paths

The board bootstrap returns workspace metadata, type definitions, counts and launch state.
Read each object list with `joppa_objects`. Read verification details with the board `process` level.
The board no longer accepts native Domain, Capability, Requirement, Consumer, filter or history levels.
Read document history by its address and revision. No list includes document bodies or revision history.
A copied ID uses the same object reader for every type name.
Verification objects also return their configured evidence context. Historical evidence reads omit current document bodies.
Search indexes current objects and their ancestor context through the same document tree.
It returns each configured type under its actual name.

## Object URLs

Open an object with `/?workspace=WORKSPACE&object=ADDRESS`.
Add `revision=N` to read one saved document revision.
Object links, configured references, and criterion links use these URLs.
The browser replaces the page and uses its normal Back and Forward history.
An object page reads its document and direct children. It does not read root lists.
Older revisions omit current children, comments, files, and verification details.
The server applies the same workspace access rules as the board.

A done Task prevents `archive_object` on its parent. Retire the Task before you archive the parent.
Each tree type can define an optional `question`. Read it from `joppa_tree` or `joppa_help` with `workspace`.
Before saving, check that the title and description answer the question. Rewrite them when they do not.
The save acknowledgement returns `object.question` for a type with a question. The server stores the question without checking meaning.
