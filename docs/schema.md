# The workspace schema

## Types and parents

Call `joppa_tree` to read the workspace schema. Use `set_tree_schema` to replace it.
The recommended recipe below has two verticals and two document types. The owner can change it.

- Product rules: `domain` → `capability` → `requirement` → `ac` → `test`.
- Technical contracts: `tech` → `requirement` → `ac` → `test`. The roots of `tech` are areas
  such as API, Database, Integrations, Platform, Security and Infra.
- `task` under `ac`. Create a Task directly before the implementation of its AC.
- `test`: one scenario that proves its AC. The title states the scenario in business language,
  with concrete numbers and no code. `body` gives the inputs and the expected outcome. `case` is
  `nominal` (the normal working path), `boundary` (the edges of limits) or `off_nominal` (errors
  and safe refusals). `command` is the exact isolated command that runs this one test. A manual check is no
  test node: it is a file on the AC with `check_result` (see [what green means](green.md)).
- `inbox`: a raw incoming signal before analysis. `source`: an owner decision, a specification
  or an external standard with a fixed version. Each AC names its Sources in `sources`.
  `requirement`, `capability`, `domain` and `tech` compute `sources` from their children.

```json
{
  "op": "set_tree_schema",
  "revision": 0,
  "reason": "Apply the recommended tree",
  "types": {
    "domain": {"title": "Domain", "badge": "DOM", "role": "group", "root": true, "parents": [], "status": "children",
      "references": {"sources": {"target": "source", "title": "Sources", "multiple": true, "from": "children"}}},
    "capability": {"title": "Capability", "badge": "CAP", "role": "group", "root": false, "parents": ["domain"], "status": "children",
      "references": {"sources": {"target": "source", "title": "Sources", "multiple": true, "from": "children"}}},
    "tech": {"title": "Tech", "badge": "TECH", "role": "group", "root": true, "status": "children", "parents": [], "color": "#3f7f8c", "title_size": 17,
      "question": "Which shared technical area (API, Database, Integrations, Platform, Security, Infra) does this root own?",
      "references": {"sources": {"target": "source", "title": "Sources", "multiple": true, "from": "children"}}},
    "requirement": {"title": "Requirement", "badge": "REQ", "role": "requirement", "root": false, "parents": ["capability", "tech"], "status": "children",
      "references": {"sources": {"target": "source", "title": "Sources", "multiple": true, "from": "children"}}},
    "ac": {"title": "Acceptance Criterion", "badge": "AC", "role": "ac", "root": false, "parents": ["requirement"], "status": "children",
      "references": {"sources": {"target": "source", "title": "Sources", "required": true, "multiple": true}}},
    "test": {"title": "Tests", "badge": "TEST", "role": "test", "root": false, "parents": ["ac"], "status": "own", "title_max_length": 400,
      "fields": {"case": {"type": "enum", "values": ["nominal", "boundary", "off_nominal"]}, "command": {"type": "text"}},
      "required_fields": {"body": "a test node needs body: the inputs and the expected outcome"},
      "question": "Which scenario, in business language and with concrete numbers, proves this criterion?"},
    "task": {"title": "Task", "role": "task", "root": false, "parents": ["ac"], "status": "none"},
    "inbox": {"title": "Inbox", "badge": "RAW", "role": "document", "root": true, "inbox": true, "status": "none", "parents": [], "field_max_length": 4096, "color": "#7664aa",
      "question": "What did the source report, and who sent it?"},
    "source": {"title": "Source", "badge": "SRC", "role": "document", "root": true, "status": "none", "parents": ["source"], "field_max_length": 4096, "color": "#8a6d3b",
      "question": "Which owner decision, specification or external standard grounds these rules, and at which version?"}
  }
}
```

To change single types, send `put_types` and `remove_types` in place of `types`:

```json
{"op": "set_tree_schema", "revision": 4, "reason": "Add an epic level under requirements",
 "put_types": {"epic": {"title": "Epic", "role": "group", "root": false, "parents": ["requirement"]}},
 "remove_types": ["release"]}
```

- `put_types` adds a type or replaces one type completely. Send all fields of that type.
- `remove_types` deletes types. The server refuses a type that present objects use, or that another type names as a parent or a reference target.
- A command with `types` cannot also have `put_types` or `remove_types`.
- A command without all three keeps every type and changes only `priorities` or `statuses`.
- `priorities` lists the values of `data.priority`, default `p0`, `p1` and `p2`. A save with another value is refused: `priority is one of ...`.

Read the current schema before you change it. The server refuses a schema that makes the
current tree invalid. A member of the workspace configures the schema. The `/tree` page only reads it.

The role of a type gives its workflow. The type name does not.

- `requirement`: specification revisions.
- `ac`: a criterion in its nearest ancestor specification.
- `group`, `document` and `task`: ordinary nodes.

`root: true` permits an object without a parent. `parents: ["*"]` permits any parent type.
A type with `inbox: true` has `parents: []`. Its name can differ from `inbox`.
A type with `source: true` marks sources: a new title, body or file of its node resets the
criteria that name it or link with it. `source: false` is not stored.
The server refuses parent cycles.
An object has one parent, or no parent when its type permits a root.
Each document revision records its schema revision.

## Required document fields

Set `required_fields` on a type. Each entry maps a `data` field to its refusal message:

```json
{"required_fields": {"source": "a signal needs a source"}}
```

A rule object requires literal markers in a text field:

```json
{"required_fields": {"body": {"contains": ["Given:", "When:", "Then:", "Evidence:"], "error": "an ac body needs Given:, When:, Then: and Evidence:"}}}
```

- Absent fields, nulls, blank text, empty lists and empty maps have no value. `0` and `false` are values.
- The value of a marker rule must be text that contains every marker, with case sensitivity, in any order.
- The marker list needs one marker or more. Markers and the error text must not be blank.
- A rule applies to each save after it. `set_tree_schema` refuses the rule when a present object lacks the value.
- HTTP returns 422 when a rule fails. MCP returns the configured error.

## Field types

Set `fields` on a type to give a data field its type:

```json
{"fields": {"env": {"type": "enum", "values": ["dev", "prod"]}, "urgent": {"type": "boolean"},
  "size": {"type": "number"}, "tags": {"type": "list"}, "note": {"type": "text"}}}
```

- A type is `text`, `number`, `boolean`, `enum` or `list`. An enum needs `values`. A list may
  name `values` that limit its elements. Another type has no values.
- A name is a lowercase letter, then up to 31 lowercase letters, digits or `_`; not title or body.
- A save refuses a value of another type: `env is one of dev, prod`, `urgent is true or false`,
  `size is a number`, `tags is a list of texts`, `note is a text`. A null or an absent field has no value.
- `set_tree_schema` refuses a rule that a present object breaks.
- The board drop-down of a typed enum field in `filters` offers each of its values.

## Computed fields

`from` sets the source of the value of a typed field. The default is `own`: the node stores it.

```json
{"task": {"fields": {"stage": {"type": "enum", "values": ["in_progress", "review", "todo", "done"]},
   "tags": {"type": "list"}}},
 "bug": {"fields": {"stage": {"type": "enum", "values": ["in_progress", "review", "todo", "done"], "from": "links"}}},
 "ac": {"fields": {"stage": {"type": "enum", "values": ["in_progress", "review", "todo", "done"], "from": "children"},
   "tags": {"type": "list", "from": "children"}}}}
```

With tasks `["web", "api"]` and `["api", "ios"]` under it, the AC gives `"tags": ["api", "ios", "web"]`.

- `children` on an enum field: the value of the present children that comes first in `values`.
- `children` on a list field: the sorted union of the values of the present children. Each
  value occurs once. A child gives its own value or its computed value, so the union climbs
  from level to level. A list with `values` keeps only the values that it names. Without a
  value in the children, the field has no value.
- `links`: the own value of the present nodes that link to the node or that it links to, the
  one that comes first in `values`. Any number of links counts.
- `parent`: the value of the nearest ancestor that has one.
- An enum field takes `children` or `links`. A list field takes `children`. Another field
  takes only `own` or `parent`. A value that `values` does not name counts not.
- `links` reads only own values, and `children` skips the children of the `parent` rule. So the
  values have no cycle.
- Each write that changes a node, its parent, its children or its links recomputes the values
  that it reaches, and the server stores them. A read gives them in `data`, and the `data`
  filter of `joppa_objects` matches them. A GIN index of the stored values finds the nodes.
- A save that sends a value of a computed field is refused:
  `stage is computed from links and cannot be set on bug`. A save without it keeps working.
- A rule change to a computed source keeps the stored values of the old rule, and a read no
  longer gives them.

## Presentation

- `color: "#RRGGBB"` colours the type on the board and in the schema diagram.
- `title_size` is an integer from 12 to 48 pixels. The default is 15.
- `badge` is a label of 1 to 8 characters without spaces. The default is the initials of the title.
- `title_max_length` is from 1 to 400 characters, default 128. A longer new title is refused. Put the context in `body`.
- `field_max_length` limits each other data value, default 1024 characters.
- `index` is a nonnegative integer. Smaller values come first in the root tabs and the diagram.

`statuses` is the ordered list of status tones with one colour for each theme:
`[{"tone": "red", "light": "#bc5d52", "dark": "#e07a70"}, ...]`. The tones are red, purple,
yellow, orange, grey and green; purple marks an archived node. The list sets the colours and
the order of the board status filter. The board lists only present nodes, so its filter omits
purple. The rollup order is fixed: red, orange, yellow, grey, green (see
[what green means](green.md)). A type has no status order: `status_order` is an unknown field.
A tone that the list omits keeps its default colours. A change without `statuses` keeps the list.
Presentation changes add a schema revision. They do not change objects or evidence.

## Status rules

Each type names its status rule in `status`: `own`, `children`, `links` or `none`. A test node
type has `own`: the newest result of its test. A criterion type has `children`.
With `status_required: true`, `set_tree_schema` refuses a type without `status`. With `false`,
the nodes of such a type have the rule `unset` and the tone orange.
The rule tone also sets the status label, for example "Children checks passed" or
"Checks failed".

## Filter fields

`filters` names up to 16 `data` fields that the board offers as filter drop-downs, for example
`["tags", "priority"]`. A name is a lowercase letter, then up to 31 lowercase letters, digits or
`_`. `title` and `body` are refused. A change without `filters` keeps the list.
The board read (`GET /api/workspaces/{w}/board`) gives `filters`: for each field, its present
values, at most 200: `[{"field": "tags", "values": [{"value": "swap", "list": true}]}]`.
A boolean field gives `false` and `true` first, then the texts in text order. `list` is true
for an element of a list field. A choice filters with the `data` node filter:
`{"tags": ["swap"]}` for a list element, `{"priority": "p0"}` for a string and
`{"mobile": true}` for a boolean. `/tree` lists the
fields under Filter fields.

## Filter dimensions

A filter field is a dimension when a field of `test_results.fields` has the same name and the
type `enum` or `ref`, for example `env` with the values `dev` and `prod`.

- The values of a dimension are the enum values, or the addresses of the present objects of
  the ref target type. The board offers each value in the drop-down of the field.
- A node applies to the values of its own `data.<field>`, a text or a list of texts. Without
  the field, it applies to the values of its nearest ancestor with the field, else to every
  value. A test node inherits the values of its criterion.
- A result without the field counts for every value. A result with another value does not count.
- A data filter with a dimension value, such as `{"env": "dev"}`, keeps the nodes that apply to
  that value. Each node shows the tone of the results that count for it, and a node counts only
  the children and the linking objects that apply. The `status` filter reads that tone too.
- Each write stores the tone of each node for each combination of values, at most 64
  combinations; the combinations of fewer dimensions come first. A read computes nothing.
- The board read gives `dimensions`: `[{"field": "env", "values": ["dev", "prod"]}]`.
- The child counts of a row stay the counts of all results.

## Test rules

The schema holds every test rule in the block `test_results`. A new workspace gets this block:

```json
{
  "statuses": {"pass": "pass", "fail": "fail", "skip": "skip", "todo": "todo"},
  "fields": [
    {"name": "message", "title": "Failure message", "type": "text", "max_length": 8192},
    {"name": "duration_ms", "title": "Duration in milliseconds", "type": "number"}
  ],
  "max_tests": 200,
  "max_results": 1000,
  "same_commit": "fail_wins",
  "reset_on_change": true,
  "green_if_all": ["pass"],
  "red_if_any": ["fail"],
  "failure_field": "message"
}
```

- `statuses` gives the status names of `joppa_record_results`. Each name counts as `pass`, `fail`, `skip` or `todo`. The map needs a name for `pass` and for `fail`.
- `fields` lists the fields of a result: `text` with `max_length`, `number`, `enum` with `values`, or `ref` with a `target` type.
- `max_tests` limits the test nodes of one parent, and `max_results` limits one record.
- `same_commit`: with `fail_wins`, a failure in any record of the newest commit decides the test. Else, and with `latest_wins`, only the newest record decides it.
- `reset_on_change` is described in [what green means](green.md).
- `green_if_all` and `red_if_any` give the outcomes that make a test node green or red.
- `failure_field` names the text field that readers show as the failure message.

The names `test`, `status`, `commit`, `actor` and `at` belong to the record. A field cannot use them.
The server refuses an unknown status name, an unknown field, a missing required field and a
value of the wrong type. A `set_tree_schema` with `test_results` sends every rule.

No rule checks the title of a test node. A `set_tree_schema` with `test_results.name` is refused:
`test_results.name was removed; the title of a test node states the scenario in business language, and a record names the test by test:<id>`.

A result field has no `board` or `applies`: `set_tree_schema` refuses them with
`board and applies were removed from result fields; filter test nodes by status`.

## Configured references

`references` maps document fields to reference rules. Type names create no rules.

```json
{"references": {"affected": {"target": "spec", "title": "Affects", "required": true,
  "multiple": true}}}
```

- `target` names a configured type, or `"*"` for any type. A wildcard needs full addresses.
- `multiple: true` takes an array of addresses. `required: true` refuses a missing or empty value.
- Duplicates and missing targets are refused. References change no parent, no readiness and no tone.
- `from: "children"` makes a computed reference. Its value is the sorted union of the
  addresses of the same field in the present children, each address once. A child gives its
  own or its computed value, so the addresses climb from level to level. The server sets the
  value, and a save that sends it is refused:
  `sources is computed from children and cannot be set on requirement`.
- A computed reference needs `multiple: true`. `set_tree_schema` refuses `required: true` or
  `allow_all: true` with it, because no save sets the value. It also refuses another `from`
  and a typed field with the same name.
- A read gives the addresses in `data` and their titles in `references`. The `data` filter
  matches them, for example `{"sources": ["source:s3"]}`.

```json
{"requirement": {"references": {"sources": {"target": "source", "title": "Sources",
  "multiple": true, "from": "children"}}}}
```

A type with `process_fields` or a reference with `verification_subject` is refused:
`process_fields and verification_subject were removed; every node shows its process facts, and a reference changes no Task scope`.

The recommended recipe defines `ac.sources` (one or more Sources, required). The types
`requirement`, `capability`, `domain` and `tech` compute `sources` from their children, so
each root gives the Sources of all criteria below it. A workspace can add
other references, for example `ac.consumer` (one consumer, required) or `task.projects`.
