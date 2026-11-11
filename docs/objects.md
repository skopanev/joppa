# Objects and workspace trees

## Common documents

Every object has an address such as `domain:billing`, `ac:payment` or `source:refunds`.
The type prefix keeps the local IDs of two types apart. Permanent public IDs stay valid.

1. Read the object with `joppa_objects`.
2. Send its current `revision` with `save_object`.
3. Send the revision of the write reply with the next write of that object.

Use `revision: 0` and omit `id` to create an object. The server makes the ID.
The server checks the revision under the workspace write lock. A stale write fails.
Each write adds a revision with the actor, time, reason and schema revision.
Retry identical input with the same `request_id` to get the same result.

An update (`revision` > 0) merges `data` into the current fields: a sent field replaces its
value, `null` removes the field, and every other field stays. `clean: true` replaces `data`
whole. A save without `parent` keeps the parent, and `parent: null` makes a root.
`relations` merges by key: a sent key replaces its `title`, `edges`, `color` and `facts`, `null` removes the
key, and every other key stays. A save without `relations` keeps them. The journal stores the
whole data, parent and relations of each save. A new edge to an archived or absent object is
refused, unless the same key of the current revision already has it.
Ordinary document fields can hold text, numbers, lists or maps.

All documents have a title and an owner. The text of every type is `data.body`: the rule of
a REQ, the observable condition of an AC (`Given / When / Then` for a flow, or a direct rule for
an invariant, limit or disclosure) and the contract of a Task. A save with `data.description`
or `data.contract` is refused: `use body instead of description; read joppa_help`.
An AC and a Task can inherit the REQ owner. A Task keeps its assignee separate from
its owner. A save with `data.decision_required` is refused. Ask an open question with `note`.

A save of a criterion (a type with role `ac`) gives its specification a new revision with the reason
`Criterion <address> changed: <reason>`. A save of a criterion with another parent moves it.
Process commands keep the document fields. Only test results make a criterion green.

Each type can define a `question`. Read it from `joppa_tree`. The title and body of an object
answer the question of its type. A save does not send the question, and the server does not check the answer.

The workspace schema gives the types, parents, required fields, status rules and test rules.
See [the workspace schema](schema.md).

## Relations and comments, not object text

Keep each object text to its own contract. Do not add other data to it.

1. Link objects with relations, never with text.
   - Send the `relations` map of `save_object`: `{"related": {"edges": [{"to": "task:<id>"}]}}`. Use the key `depends_on` for a dependency.
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
See [relations and other workspaces](links.md) for the registry, dependencies, flows and remote links.

## Inbox items and Sources

Keep three items apart:

1. An Inbox item is a raw incoming signal before analysis. Its type has `inbox: true`.
2. A Source is an owner decision, a specification or an external standard with a fixed version.
3. A comment (`op: "note"`) is a discussion inside a card. It creates no document.

Save a signal as an Inbox item with `parent: null`. The server refuses an Inbox item with a parent.
Keep the original text and language of the signal. A missing `data.status` means `unprocessed`.

```json
{"op": "save_object", "kind": "inbox", "revision": 0, "parent": null,
 "data": {"title": "Customer feedback", "owner": "owner", "body": "Original source text"},
 "relations": {"related": {"edges": [{"to": "domain:billing"}]}}, "reason": "Record customer feedback"}
```

Analyze the Inbox item, then do these steps:

1. Save the lasting content as a Source and as REQ and AC nodes.
2. Give the Inbox item a `related` edge to each new node.
3. Archive the Inbox item with `archive_object`.

A Source keeps the verbatim text and names its version. A Source can have a parent Source,
for example a section of a specification. A changed Source gets a new revision.
Each AC names its Sources in `data.sources`, a list of addresses: `"sources": ["source:refunds"]`.
When the schema makes `sources` required, the server refuses an AC without a Source.
Set `sources` only on the AC. Do not copy them to the REQ.

## Tests of a criterion

A test is a node of a type with role `test` under its criterion. Save one node
for each test with `save_object`:
`{"kind": "test", "parent": "ac:x", "data": {"title": "<the scenario in business language>", "owner": "..."}}`.
The title states the scenario with concrete numbers and no code. The schema gives the other
fields, such as `body`, `case` and `command`. A record names the test by `test:<id>`, which binds
that one test node. A title binds every present test node with that title.
No criterion, no test: every test in the repository has a present test node, and a full-run record of
`joppa_record_results` gives `"unbound": []` before a merge. See [what green means](green.md)
for the fields, the Test-5 checklist, the tones and the rollup.

## Before implementation

Score each REQ and AC with the 11111 checklist and the Reuse gate before implementation.
An independent Reviewer agent with another alias scores the node. The agent that wrote the
node never scores it. Prepare first:

- Read the full node and its parent.
- Read the full verbatim text of all cited and applicable sections in every linked Source,
  on the node or its ancestors. Never read only a summary.
- Inspect the existing mechanisms, schemas and contracts of the target system in the codebase and the Tech root.

Score five rules, in the order G A U M V, then apply gate 6, Reuse. Each rule is 1 or 0.

1. Grounded (G). 1: every condition, value, limit, timing and branch of the node appears
   explicitly in the linked Source text. Code and tests are never a valid Source on their own.
   To record an existing baseline schema or interface, link an explicit baseline decision as Source.
   Each new entity, field, state or endpoint needs a new Source.
   0: no Source is linked, the Source text was not read, or the node adds an assumption, number
   or structure that the Source does not have. On 0: STRIP the text without a Source, or ask for the missing Source.
2. Atomic (A). 1: one indivisible behavioral rule, or one cohesive entity or endpoint schema.
   The fields of one schema stay in one node. Split independent conditions, limits and state
   transitions that can pass or fail separately. 0: two or more independent conditions,
   outcomes or checks. On 0: SPLIT into the listed atomic nodes.
3. Unambiguous (U). 1: the node and the Source allow one interpretation and one implementation
   path. Every term, boundary and starting point has a definition. 0: two or more valid readings,
   a missing reference point, or several implementation choices. On 0: ASK_OPTIONS, one closed
   question for the person who decides, with concrete options, the simplest first.
4. Minimal (M). 1: the node specifies the behavior or structure of the target software itself,
   not manual human or governance actions, unless the Source explicitly mandates automation.
   Also, the removal of any entity, state, attribute or constraint, or its replacement with an
   existing mechanism, would violate the Source. 0: the node pushes a manual human duty into
   software, or adds entities, tables, states, fields or mechanisms when existing mechanisms
   satisfy the Source. On 0: STRIP, or ASK_OPTIONS between the existing mechanism and a new one.
5. Verifiable (V). 1: concrete inputs or triggers and an observable outcome, or an exact schema
   contract, that a finite deterministic automated test proves Pass or Fail. 0: subjective,
   dependent on an unconfirmed external rule, or not deterministic. On 0: REWRITE and name
   the missing trigger or outcome.

6. Reuse (R). 1: the node and its implementation plan use the ready shared types, packages,
   libraries, helpers, modules and contracts of the codebase and the Tech root. 0: the plan
   duplicates an existing mechanism, or the search was not done. Search with the code graph
   index (codebase-memory, CBM: `search_graph`, `search_code` and `SIMILAR_TO`) when it is
   connected. On 0: ASK_OPTIONS between the existing mechanism and a new one, or REWRITE.

The verdict gives the bits G A U M V and R. APPROVE only 11111 with R, and tie the verdict to
the exact revisions of the node, its Sources and its `depends_on` targets. Else, HOLD and give
STRIP, SPLIT, ASK_OPTIONS or REWRITE with the exact fix for each 0. On a HOLD, the Writer agent
asks the owner one closed question in the chat, updates the Source and the AC, and the Reviewer
agent scores again. The server does not check these roles.

The Reviewer agent scores the AC before any test of it exists. After APPROVE, two independent
agents work on the same AC:

- One agent writes the code for the AC with its Tasks. One AC can have one Task or more.
- Another agent writes the acceptance Test nodes of the AC as a black box, from the AC and its
  Sources. The coder does not see these tests first, so it cannot fit the code to them.

## Tasks

Create a Task only under an AC, directly before you implement that AC. Do not create Tasks
in advance while the tree is filled or agreed: the tree changes too fast.
Create a Task with `save_object` and a type whose role is `task`. The `body` is the contract.
A Task has no purpose: `save_object` refuses `data.purpose` with
`a Task has no purpose; read joppa_help`. A member who can write can change any Task.

- `data.assignee` names the member ID or the agent alias that does the work, such as `lane3`.
  A list item of `joppa_objects` and a board row show it.
- `data.commit` gives the Git sha of the finished work.
- A typed field of the schema, such as `data.stage`, gives the state of the Task. `archive_object` ends the Task.
- Read Tasks with the node filters of `joppa_objects`, for example `{"kind":"task","assignee":"alex","ready":true}`.

## Archive and restore

`archive_object {target, revision, reason}` archives an object that has no live children.
`restore_object` takes the same fields and needs a parent that the schema permits.
Both keep IDs, relations, fields and comments. IDs are never used again. `restore_object`
refuses a node with a relation that the registry no longer has: `declare relation <key> first`.

## Reads

`joppa_objects` without `address` lists root objects. `parent` lists the children of one
object, and `parent: "*"` lists all current objects in pages. `kind`, `data`, `related_to`
and `note_status` filter the list. Lists give no bodies and no history.

`address` gives one current object: `revision`, `parent`, `data`, `comments`, the
`relations` with titled edges and the incoming `relation_groups`. `relations_of` gives the
edges of an object in pages of 25, and `in_relation` the nodes of one relation. `revision: N` reads one saved revision. `history: true` gives the revision
headers. `address` also takes a copied public item ID, such as `joppa-R-DP6KXM62`. See [read contract](read-contract.md) and [read detail](read-detail.md).

Open an object in the board with `/?workspace=WORKSPACE&object=ADDRESS`. Add `revision=N`
to read one saved revision. Open a flow with `/?workspace=WORKSPACE&view=flow&relation=KEY`,
and add `&node=ADDRESS` (an address or a public item ID) to pick one node in that flow.
