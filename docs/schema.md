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

Read the current schema before you change it. The server refuses a schema that makes the
current tree invalid. A member of the workspace configures the schema. The `/tree` page only reads it.

The role of a type gives its workflow. The type name does not.

- `task`: claim, release, completion and queue operations.
- `requirement`: specification revisions and owner acceptance.
- `ac`: a criterion in its nearest ancestor specification.
- `group` and `document`: ordinary nodes.

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
- `title_max_length` is from 1 to 256 characters, default 128. A longer new title is refused. Put the context in `body`.
- `field_max_length` limits each other data value, default 1024 characters.
- `index` is a nonnegative integer. Smaller values come first in the root tabs and the diagram.

`statuses` is the ordered list of status tones with one colour for each theme:
`[{"tone": "red", "light": "#bc5d52", "dark": "#e07a70"}, ...]`. The tones are red, purple,
yellow, orange, grey and green. A higher entry wins over a lower one in a children rule.
A tone that the list omits keeps its default colours. A change without `statuses` keeps the list.
Presentation changes add a schema revision. They do not change objects or evidence.

## Status rules

Each type names its status rule in `status`: `tests`, `children`, `links` or `none`.
With `status_required: true`, `set_tree_schema` refuses a type without `status`. With `false`,
the nodes of such a type have the rule `unset` and the tone orange.
A rule tone that differs from the process tone also sets the status label, for example
"Children checks passed" or "Checks failed".
`process_fields` lists `confirmed` for a requirement or a criterion and `acceptance` for a
requirement. An object shows only the process fields that its type lists.

## Test rules

The schema holds every test rule in the block `test_results`. A new workspace gets this block:

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

- `statuses` gives the status names of `joppa_record_results`. Each name counts as `pass`, `fail`, `skip` or `todo`. The map needs a name for `pass` and for `fail`.
- `fields` lists the fields of a result: `text` with `max_length`, `number`, `enum` with `values`, or `ref` with a `target` type.
- `name` gives the form of a test name. `max_tests` limits a test list, and `max_results` limits one record.
- `same_commit`: with `fail_wins`, a failure in any record of the newest commit decides the test. With `latest_wins`, only the newest record decides it.
- `reset_on_change` and `draft_on_text_change` are described in [what green means](green.md).
- `green_if_all` and `red_if_any` give the outcomes that make a criterion green or red.
- `failure_field` names the text field that readers show as the failure message.

The names `test`, `status`, `commit`, `actor` and `at` belong to the record. A field cannot use them.
The server refuses an unknown status name, an unknown field, a missing required field and a
value of the wrong type. A `set_tree_schema` with `test_results` sends every rule.

An `enum` or `ref` field with `"board": true` gives each value its own criterion status.
A result without the field counts for every value. `applies` names the reference field of a
criterion that lists the values that apply. A reference with `"allow_all": true` also takes
`all`. A value that does not apply has the status `n/a`. The criterion is red when one value
is red, green when every value is green, and grey in other cases. `joppa_tests` gives each
value in `values`. The board has one filter for each board field.

## Configured references

`references` maps document fields to reference rules. Type names create no rules.

```json
{"references": {"affected": {"target": "spec", "title": "Affects", "required": true,
  "multiple": true, "verification_subject": false}}}
```

- `target` names a configured type, or `"*"` for any type. A wildcard needs full addresses.
- `multiple: true` takes an array of addresses. `required: true` refuses a missing or empty value.
- Duplicates and missing targets are refused. References change no parent and block nothing.
- `verification_subject: true` adds the reference to the Task scope check and the subject filter.

The default recipe defines `ac.consumer` (one consumer, required) and `task.projects`.
`launch_blocker: true` on a type makes its `before_launch` entries launch conditions.
Check evidence and owner acceptance must cover each such entry.
