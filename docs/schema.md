# The workspace schema

## Types and parents

Call `joppa_tree` to read the workspace schema. Use `set_tree_schema` to replace it:

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

- `requirement`: specification revisions and owner acceptance.
- `ac`: a criterion in its nearest ancestor specification.
- `group`, `document` and `task`: ordinary nodes.

`root: true` permits an object without a parent. `parents: ["*"]` permits any parent type.
A type with `inbox: true` has `parents: []`. The server refuses parent cycles.
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
- The server checks `question_checked` before the required fields.
- A rule applies to each save after it. `set_tree_schema` refuses the rule when a present object lacks the value.
- HTTP returns 422 when a rule fails. MCP returns the configured error.

## Presentation

- `color: "#RRGGBB"` colours the type on the board and in the schema diagram.
- `title_size` is an integer from 12 to 48 pixels. The default is 15.
- `badge` is a label of 1 to 8 characters without spaces. The default is the initials of the title.
- `title_max_length` is from 1 to 400 characters, default 128. A test name can have 400 characters. A longer new title is refused. Put the context in `body`.
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
values in text order, at most 200: `[{"field": "tags", "values": [{"value": "swap", "list": true}]}]`.
`list` is true for an element of a list field. A choice filters with the `data` node filter:
`{"tags": ["swap"]}` for a list element, `{"priority": "p0"}` for a string. `/tree` lists the
fields under Filter fields.

## Test rules

The schema holds every test rule in the block `test_results`. A new workspace gets this block:

```json
{
  "statuses": {"pass": "pass", "fail": "fail", "skip": "skip", "todo": "todo"},
  "fields": [
    {"name": "message", "title": "Failure message", "type": "text", "max_length": 8192},
    {"name": "duration_ms", "title": "Duration in milliseconds", "type": "number"}
  ],
  "name": {"separator": " > ", "min_parts": 2, "file_extension": true, "max_length": 400},
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
- `name` gives the form of a test name. `max_tests` limits the test nodes of one parent, and `max_results` limits one record.
- `same_commit`: with `fail_wins`, a failure in any record of the newest commit decides the test. With `latest_wins`, only the newest record decides it.
- `reset_on_change` is described in [what green means](green.md).
- `green_if_all` and `red_if_any` give the outcomes that make a test node green or red.
- `failure_field` names the text field that readers show as the failure message.

The names `test`, `status`, `commit`, `actor` and `at` belong to the record. A field cannot use them.
The server refuses an unknown status name, an unknown field, a missing required field and a
value of the wrong type. A `set_tree_schema` with `test_results` sends every rule.

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

A type with `process_fields` or a reference with `verification_subject` is refused:
`process_fields and verification_subject were removed; every node shows its process facts, and a reference changes no Task scope`.

The default recipe defines `ac.consumer` (one consumer, required) and `task.projects`.
`launch_blocker: true` on a type makes its `before_launch` entries launch conditions.
Check evidence and owner acceptance must cover each such entry.
