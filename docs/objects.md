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
A link to an archived or absent object is refused, unless the current revision already has the same link.
A save of a criterion also gives its specification a new revision. Its reason is `Criterion <address> changed: <reason>`.
Ordinary document fields can contain additional text, numbers, lists or maps.
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
    "note": {"title": "Note", "role": "document", "root": true, "parents": [], "inbox": true}
  }
}
```

To change single types, send `put_types` and `remove_types` in place of `types`:

```json
{
  "op": "set_tree_schema",
  "revision": 4,
  "reason": "Add an epic level under requirements",
  "put_types": {
    "epic": {"title": "Epic", "role": "group", "root": false, "parents": ["requirement"]}
  },
  "remove_types": ["release"]
}
```

`put_types` adds a type or replaces one type completely. Send all fields of that type. The other types stay.
`remove_types` deletes types. The server refuses a type that present objects use, or that another type names as a parent or a reference target.
A command with `types` cannot also have `put_types` or `remove_types`.
A command without all three keeps every type and changes only `priorities`, `statuses` or `status_colors`.
Preserve types and parent rules needed by existing objects.
The server rejects a schema that makes the current tree invalid.
Read the current schema before you replace it.
Agents and the workspace owner can configure the schema.
The separate `/tree?workspace=...` page only reads it.

Use custom types with any supported role. A `task` role supplies claim, release, completion and queue operations.
A `requirement` role supplies specification revisions and owner acceptance.
An `ac` role supplies a criterion in its nearest ancestor specification.
These roles work with any configured type name, including `spec`, `epic`, `measure` and `rule`.
A type name does not select its workflow. Former default names can also use ordinary document or group roles.
All ordinary fields follow the same document rules. Reference fields come only from the active schema.
Custom workflow commands use full object addresses in `req` and `ac`.
Two types can use the same local identifier without sharing test evidence.
A criterion can have intermediate grouping objects above it.
A parent change cannot move existing criterion evidence to another specification.

Rename their display titles and set their allowed parents.
`root: true` permits an object without a parent.
`parents: ["*"]` permits any parent type. A type with `inbox: true` has `parents: []`.
The server rejects parent cycles.

The default schema uses REQ → AC → Task. These parent rules are configuration.
Schemas can omit these types. Task parents can name any configured type, including Task itself.
Objects have one parent or no parent when their type allows roots.
Move active Tasks only to parents permitted by the current schema.
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
Use a rule object to require literal markers in a text field:

```json
{"required_fields": {"description": {"contains": ["Given:", "When:", "Then:", "Evidence:"], "error": "an ac description needs Given:, When:, Then: and Evidence:"}}}
```

The value must be text and contain every configured marker.
The server compares markers with case sensitivity. Their order does not matter.
The rule checks the text form. An agent checks its meaning.
The marker list must contain at least one marker. Markers and error text must not be blank.
The server rejects unknown fields in a rule object.
The existing string rule keeps its value presence checks.
For a type with `question`, supply the exact `question_checked` first.
The server checks that question before it checks required fields.
The default map is empty. The type name does not select this rule.
Rules apply to creation and each later document revision.
The server rejects a schema if a present object lacks a required value.
Archived and deleted objects do not block the rule.
Supply missing values before you enable the rule.
HTTP returns 422 if a required field rule fails. MCP returns the configured error.
Failed writes preserve the journal, revisions and request IDs.
Historical reads retain the original document and schema revision.

## Links and comments, not object text

Keep each object text to its own contract. Do not add other data to it.

1. Link objects with links, never with text.
   - Send the `links` array of `save_object`: `[{"target": "task:<id>", "relation": "related"}]`. Use `depends_on` for a dependency.
   - Use the reference fields of the schema, for example `consumer` or `projects`.
   - Use `set_remote_links` for an object of another workspace.
   - Do not paste lists of Tasks, tickets or addresses into `description`, `body` or `contract`.
2. Add findings, progress and supplements as comments, never as object text.
   - Send `{"op": "note", "target": "<kind>:<id>", "id": "<unique id>", "text": "..."}`.
   - A comment keeps its author and time. It adds no object revision and does not reset tests.
   - The field limit of a type (`field_max_length`, default 1024 characters) does not apply to a comment. Over 1 kb: prefer attach as a file.
   - Do not rewrite or extend `description`, `body` or `contract` with these texts.

One `joppa_batch` can save an object and add its comments atomically.

## Links and dependencies

An object can link to many objects of any type in the same workspace.
A Note can link to two Domains. A Domain can link to another Domain.
A link does not change tree parentage.
In text, write a reference as [Title](kind:id). The board shows the title as a link to the object.

One object read gives `links`, its outgoing links with titles, and `link_groups`: a count for each
direction, relation and kind of the other object. The MCP read keeps only the incoming counts,
because `links` already gives the outgoing links. It does not list the incoming links. Read the
links in pages with `joppa_objects`. `direction`, `relation` and `kind` are optional filters:

```json
{"workspace": "product", "links_of": "domain:billing", "direction": "incoming", "relation": "related", "kind": "note", "limit": 50}
```

The reply gives `items` in the order direction, relation, address, and `next_cursor`. Send
`cursor` with that value for the next page. `next_cursor` is null on the last page. In an item, `address`, `title`, `kind` and
`present` describe the other object now. `revision` is the target revision that the link pins, or
null for a link without a pin. It is not the current revision of the other object: read that object
to get its current revision.

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

Create new incoming information as `kind: "note"`. A Note has no parent: send `parent: null`
and link it with `related` to each object that it informs. The server refuses a Note with a
parent. Omit `id`: the server makes one.

```json
{
  "op": "save_object",
  "kind": "note",
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
The `note` operation adds a comment. It does not create a Note document.

Link a REQ to a Note with `depends_on` and the Note revision that informed the wording.
A later Note change then produces a review hint.
The agent reads that Note before it changes the requirement.
There is no separate comment-to-Note promotion process.

## Compact MCP reads

MCP reads give small replies. `docs/read-contract.md` and `docs/read-detail.md` give every shape with JSON. MCP replies give no `item_id`. An MCP list reply holds at most 256 KB; a larger list is refused. One object comes in full.

- A `joppa_objects` list gives at most 25 items a page: `address`, `title`, the status tone, `children` when above 0, and `total`. Follow `next_cursor`; a missing `next_cursor` means the last page.
- `data: {"tags": ["kyc"]}` lists the objects whose `document.data` contains that JSON object, at every level when `parent` is omitted.
- Name more item fields with `fields`, as dotted paths such as `process.planning_claim`. Each named path comes back, also when it is empty. An unknown path is refused.
- `joppa_objects` with `address` gives the current object: `revision`, `parent` and `data` exactly as stored, `comments` as `{items, next_cursor, total}` pages of 25, and all titled outgoing `links` as one list. `save_object` takes the links back as they are. Send `comments_cursor` for the next comment page. `revision: N` reads one revision. `history: true` gives the revision headers `{revision, at, actor, reason}` as `items`, in pages of 25 with `total`.
- `address` takes up to 25 addresses separated by commas. 2 to 25 addresses give `{items}`, and each item gives its `address`. With `address`, `fields` gives only the named paths; a path may name a detail key such as `links`. A bare document field such as `body` names `data.body`.
- Keys with `null`, `""`, `[]` or `{}` are omitted everywhere outside `document`, `data`, `command` and `remote_links`. `false` and `0` stay. A missing `tests` key means no tests.
- `joppa_tests` reads at most 100 criteria in one call. Send `parent` (direct children, or `*`) and `kind` (default `ac`) in place of `ac` to read every criterion in pages of 100.

## Narrow reads

`joppa_objects` without an address lists root objects.
Supply `parent` to list one object's children.
Supply `parent: "*"` to list all current objects, with pagination.
Use `kind` to filter that list.
Lists contain titles, owners, revisions and readiness hints. They do not contain document bodies or history.

Supply `address` to read one current document, its comments, attachments and relation titles.
Use `history: true` to read the revision headers.
Use `revision: N` to read exactly one historical document.
Historical reads omit current status, incoming links, comments and attachments.
Outgoing links identify their metadata as current. Removed targets remain readable and have a historical label.
Pinned dependencies open the saved target revision.
Browser cards use the same lazy read paths.
The custom tree supports filters by type, owner and status. Filters read summaries, not document bodies.
The database indexes workspace, parent, type and incoming links.

Task process commands use the Task object address and its own revision.
`joppa_objects.revision` always selects the addressed document's revision.
Read the current Task before you claim, release, review, retire or complete it.
A claim persists until an explicit recorded action changes its holder or work state.
Only the holder completes the work. The holder or workspace owner can release it.
Earlier journal entries, claim metadata, documents and work remain readable.

### Context for external planners

Use object addresses as canonical identifiers for a configured tree.
The `id` field of a common object is its local identifier. Its `address` includes the type prefix.
Resolve a public identifier with `joppa_search`, the identifier as `query`.
The reply gives only that object, with `exact_id: true`. Then use `joppa_objects` for the current document.

Read a selected object's ancestors through `document.parent`, one object at a time.
Stop when the parent is null. Use the workspace schema to identify semantic roles.
Do not require Domain or Capability names, a fixed depth, or grouping confirmation fields.
Read configured references from the common document and its `references` metadata.

Record each source address and its document revision in the implementation plan.
Read linked documents at their pinned revision when the link supplies one.
Before publishing work, compare the relevant current revisions with the recorded source revisions.
Unrelated workspace events do not invalidate an unchanged source contract.
A write reply gives the journal `position` of the write; MCP reads give no snapshot position. A position does not replace source revision checks.
Reject unknown, inaccessible or cyclic parent context with a clear error.
Never treat missing context as successful discovery.

### Removed commands

Live writes refuse the removed operations, for example `domain`, `revise`, `task`, `describe`,
the check-run operations and `set_note_status`.
The refusal names the replacement, usually `save_object`. Only journal replay reads these operations.
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

Use `joppa_tasks` for the current queue.

## Note processing and type presentation

Set `inbox: true` on any schema type to hold incoming information.
An omitted flag disables the queue. The type name does not supply a default. Missing `data.status` means `unprocessed`.
Use `decide_note` with `target`, the object `revision`, `outcome` and `reason`. A final
outcome sets the status `processed`. The server creates the next object revision.
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
Each type can set `title_preview_length`, from 1 to 256 characters. Omit it to use 128 characters.
This value is also the maximum length of a new title. The server refuses a longer title, so put the context in `body` or `description`.
Older titles can be longer. The board shows a preview that ends with an ellipsis, and the complete title stays in the Details section.
Read the current schema before `set_tree_schema`. Preserve the other type fields.

The schema can set `statuses`, the ordered list of status tones with a colour for each theme:
`[{"tone": "red", "light": "#bc5d52", "dark": "#e07a70"}, {"tone": "grey", ...}, {"tone": "green", ...}]`.
The tones are red, purple, yellow, orange, grey and green. Each colour is `#RRGGBB`. Each tone appears once.
A higher entry wins over a lower one: the list order is the default order of a children rule.
A tone that the list omits keeps its default colours. A schema change without `statuses` keeps the current list.
joppa_tree shows the list that applies. The older `status_colors` map still works and sets one colour for both themes.
The schema holds every test rule in the block `test_results`. The code reads the rules only from this block:

```json
{
  "statuses": {"pass": "pass", "fail": "fail", "skip": "skip", "todo": "todo"},
  "fields": [
    {"name": "message", "title": "Failure message", "type": "text", "max_length": 8192},
    {"name": "duration_ms", "title": "Duration in milliseconds", "type": "number"},
    {"name": "app", "type": "enum", "values": ["ios", "android"], "board": true}
  ],
  "name": {"separator": " > ", "min_parts": 2, "file_extension": true, "max_length": 400},
  "max_tests": 200,
  "max_results": 1000,
  "same_commit": "fail_wins",
  "reset_on_change": true,
  "draft_on_text_change": true,
  "draft_fields": ["title", "description"],
  "green_if_all": ["pass"],
  "red_if_any": ["fail"],
  "failure_field": "message"
}
```

- `statuses` gives the status names that `joppa_record_results` accepts. Each name counts as the outcome `pass`, `fail`, `skip` or `todo`. The map needs a name for `pass` and a name for `fail`.
- `fields` lists the fields of a result. A field has the type `text`, `number` or `enum`. A `text` field has `max_length`: a longer value keeps its first `max_length` characters. An `enum` field lists its `values`.
- `name` gives the form of a test name in a test list. `max_tests` limits a test list, and `max_results` limits one record.
- `same_commit`: with `fail_wins`, a failure in any record of the commit of the newest record decides the test. With `latest_wins`, only the newest record decides it.
- `reset_on_change`: a change at or above a criterion makes the earlier records stop counting.
- `draft_on_text_change`: a change of a `draft_fields` field makes the saved test list a draft until it is saved again.
- `green_if_all` and `red_if_any` give the outcomes that make a criterion green or red. An acceptance needs the `green_if_all` outcomes on its commit.
- `failure_field` names the text field that readers show as the message of a failure.

The names `test`, `status`, `commit`, `actor` and `at` are part of the record. A field cannot use them.
The server refuses a result with an unknown status name, an unknown field, a missing required field or a value of the wrong type.
A `set_tree_schema` command with `test_results` sends every rule. A command without `test_results` keeps the current block.
A new workspace gets the block above.
Each type of the schema names its status rule in `status` (`tests`, `children`, `links` or `none`) and its process fields in `process_fields`. No rule comes from the role of a type.
A rule tone that differs from the process tone also sets the status label, for example "Children checks passed" or "Checks failed". A grey rule tone keeps a process label that is not green or red, because that label names the next step.
The schema flag `status_required` decides a type without `status`. With `true`, `set_tree_schema` refuses the type. With `false`, the schema saves it, and its nodes have the rule `unset` and the tone orange.
The test list of a node is the field `tests` of its document, a list of test names. Any type with `status: "tests"` takes it, also a domain. The node can have children, and only its tests give its tone.
`save_object` with `data.tests` saves the list. The short command `set_ac_tests {target, tests, revision, reason}` saves only that field as an ordinary revision of the document. Its `revision` is the document revision, which `joppa_tests` gives as `tests_revision`.
A new list resets the criterion, and so does a save of the same list with `set_ac_tests`. A revision that changes only the list keeps the pins of Tasks and asks for no Task review, and it gives the specification no new revision.
A node with `status: "tests"` and no list has an empty list at its document revision: `joppa_tests` gives that revision as `tests_revision`, and `set_ac_tests` needs it.
An object read gives `contract_revision` next to `current_revision`, and each list row gives it next to `revision`. A revision that changes only the list keeps the contract revision of the revision before it. A pin on a non-Task object is current when `contract_revision <= pin <= current_revision`, also for a link to another workspace.
A result field can have the type `ref` with a `target` type, such as `asset`. Its value names a present object of that type by id or address. A new object of the type is a new value, with no change of the schema.
An `enum` or `ref` field can set `"board": true`. Each value of a board field gets its own criterion status from the results with that value. A result without the field counts for every value.
A board field can set `applies`, the reference field of a criterion that lists the values that apply to it. A reference with `"allow_all": true` also takes the explicit value `all`. A value that does not apply has the status `n/a`: it does not count, and the board shows "Not for" that value.
The criterion status combines the values: red when one value is red, green when every value is green, and grey otherwise. A value without results is grey.
`joppa_tests` shows the status of each value in `values`. Each object row stores `value_tones`, the tone of the row for each `field:value`.
The board offers one filter for each board field. With a value selected, each row dot shows the tone for that value.
A type can set `process_fields`: `confirmed` for a requirement or a criterion, and `acceptance` for a requirement.
An object shows only the process fields that its type lists. A type without `process_fields` shows no confirmation and no acceptance.
Presentation changes create a schema revision. They do not change object revisions, parents, or verification evidence.

## Other workspaces

One product can use several workspaces, for example a product workspace and one workspace for each component.
Each workspace keeps its own tree, search, History and Tasks.

A node stands for another workspace when its data has `workspace` with the ID of that workspace.
The server refuses the own workspace, a workspace name in place of its ID, and a workspace that the writer cannot read.
A read of the node gives `child_workspace`: the status and the green, red and total counts of the criteria of that workspace.
A reader without access to that workspace sees only `access: false`. This status is for display. It is not evidence for a criterion of this workspace.

`set_remote_links {target, revision, links, reason}` replaces the links of one object to objects of other workspaces.
`revision` is the `remote_links_revision` that you read, 0 when the object has no remote links. Each write raises it.
A link can target any object of any workspace that the writer can read.
Each link has `workspace` (its name or ID; the server stores the ID), `target` (kind:id), `relation`, `revision` and `blocking`.
A blocking link needs `revision`, the target revision that the work depends on:

```json
{"op": "set_remote_links", "target": "ac:checkout", "revision": 0, "reason": "Checkout uses the auth API",
 "links": [{"workspace": "backend", "target": "requirement:auth-api", "relation": "depends_on", "revision": 3, "blocking": true}]}
```

The server refuses a target that is absent or archived, a revision above the current one, and a workspace that the writer cannot read.
A blocking link that closes a dependency cycle across workspaces is refused.
A read of the object gives `remote_links`, each with its current `status`: `access`, `present`, `current_revision`, `review_required` and `satisfied`.
`review_required` is true when the target has a revision above the pinned one, or when the target is archived or absent. The object then has `remote_review_required: true`, and the board shows a review note.
A new, changed or removed pin resets the criteria below the object. Save the links again with the new revision after the review.
A plan or a Task claimed before such a change cannot finish on the old pin: finish_planning and complete_task refuse with "the remote links changed after the claim; release and claim again".
A blocking link makes the object and the work below it wait until the target is ready: its pin is current, every criterion at and below it is green, no node there waits for a local dependency, and each blocking remote link that applies there is ready too.
Without read access, a status has `access: false` and null values, and a blocking link counts as not ready.
Claims, completions (`complete_task`) and acceptances (`accept_commit`) check this wait with the access of the writer. An acceptance also checks the blocking remote links of the nodes below its target. They lock every workspace that a chain of blocking remote links reaches, in sorted ID order, so a record of results anywhere in the chain waits or comes first. A write that reaches a workspace outside its locks fails and asks for a retry.
An empty `links` list removes the links of the object. `workspace_key` in joppa_tree is the immutable ID of the workspace. joppa_tree gives it only when the request named an alias of the workspace; the MCP object read never gives it.

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
The default recipe defines `ac.consumer` (one consumer, required) and `task.projects`. Each workspace can replace or remove them.

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

Type titles and colors remain workspace settings. For example, the `note` type can display the title `Input`.

Set `launch_blocker: true` on a type to use its `before_launch` entries as launch conditions.
Current check evidence and owner acceptance must cover each selected entry.
Use `launch_blocker: false` to disable this behavior. An omitted flag also disables it.

A task purpose can be any nonempty label. The `decision` label retains its configured decision behavior.

## Work and verification status

A Task has the status rule `none`: its tone comes only from its own state. An open Task is grey, a Task in progress is yellow, and a done Task is green.
`complete_task {id, revision, evidence, commit}` sets done. `commit` is the git sha where the Task is ready, and the Task keeps it.
A Task has no verification of its own. The test status shows on the criterion, and a red criterion does not change a done Task.
A blocking dependency on a requirement waits for its current checks to pass. Owner acceptance remains separate.
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
For `save_object`, send `question_checked` with the exact configured question. The server refuses an absent or different value before recording the operation.
The save acknowledgement returns `object.question` for a type with a question. The server stores the question without checking meaning.
