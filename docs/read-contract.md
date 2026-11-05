# MCP read contract

This file gives the exact shape of the MCP reads. It matches `requirement:read-contract`.
MCP reads have one shape. There is no `verbose` switch.

## Common rules

- A page holds at most 25 items. A missing `next_cursor` means the last page.
- Send the `next_cursor` value back as `cursor` (lists) or `comments_cursor`.
- Keys whose value is `null`, `""`, `[]` or `{}` are omitted, except inside `document`,
  `data`, `command` and `remote_links`. Those stay byte for byte as stored. `false` and `0` stay.
- A top-level `items: []` stays when a list is empty.
- Read an omitted key as empty: no value, no list, no map, `0`.
- No key repeats another key, the address or the request. No read gives `workspace` or
  a snapshot `position`, and no read repeats the `address`, `links_of`, `ac`, `task` or
  `target` that the request sent. A detail or revision read gives `address` only when the
  request named an alias of the object.
- No key gives a default value: a flag shows only when it is `true`, and a link relation
  only when it is not `related`. Read a missing flag as `false`.
- An MCP reply with `items` holds at most 256 KB (262144 bytes). A larger list is refused:
  `reply exceeds 256 KB; narrow the request with fields, fewer addresses or a smaller limit`.
  A read of one object comes in full, because one field can hold 256 KiB.
- Each MCP read takes a public item ID such as `joppa-R-DP6KXM62` where it takes an
  address: `address`, `parent`, `related_to` and `links_of` of `joppa_objects`, and `parent`
  of `joppa_search`. A read of one object by its item ID gives its
  `address`. An unknown item ID is refused: `unknown object`.
- A public item ID ends with an 8-character code of uppercase letters and digits. A copy
  with a longer code reads the object of its first 8 characters.
- `workspace` is optional in `joppa_objects` when the call names a public
  item ID. The call reads the workspace of its IDs. The IDs of one call belong to one
  workspace: `the public item IDs of one call belong to one workspace`. An ID that the caller
  cannot read is `unknown object`. A call without an ID and without `workspace` is refused:
  `missing workspace`.
- MCP replies give no `item_id`. To resolve a copied public ID, send it as `query` to
  `joppa_search`: the reply gives only that object, with `exact_id: true`.

## Limits for clients that send a larger `limit`

| Tool | `limit` above the page size | Paging |
| --- | --- | --- |
| `joppa_objects` lists, also with `fields`, `related_to`, `note_status` | clamped to 25, never refused | `next_cursor` and `total` |
| `joppa_objects` with `links_of` | clamped to 25 | `next_cursor` and `total` |
| `joppa_objects` with `history: true` | fixed pages of 25 | `next_cursor` and `total` |
| `joppa_objects` detail comments | fixed pages of 25 | `comments_cursor` |
| `joppa_timeline`, `joppa_history` | clamped to 25, default 25 | `next_cursor` and `total` |
| `joppa_search` | clamped to 25, default 10 | `next_cursor` and `total` |
| `joppa_list` | fixed pages of 25 | `next_cursor` and `total` |

Every MCP list reply is `{items, total, next_cursor}`. `total` counts every matching entry,
also on later pages. Send `next_cursor` back as `cursor`; a missing `next_cursor` means the
last page. `limit` is 1 to 25 in every MCP list schema. Follow `next_cursor` for more.

## List mode: `joppa_objects` without `address`

Request:

```json
{"workspace": "w", "parent": "requirement:r", "limit": 25}
```

`parent` is a node address (every descendant), `*` for every node,
or absent for the roots. The [node filters](#node-filters) narrow the list. Reply:

```json
{
  "total": 27,
  "next_cursor": "ac:a3",
  "items": [
    {"address": "ac:a", "title": "Each saved task has an owner", "status": "green"},
    {"address": "ac:a1", "title": "A refused save stores nothing", "status": "grey"}
  ]
}
```

- `status` is the status tone that the last write stored in the row. One query reads it.
- `fields` `["children"]` gives the number of present children of each item, also `0`.
- `fields` `["links"]` gives the stored links of each item, for example
  `[{"target": "ac:b", "relation": "depends_on", "blocking": true}]`. A link with a pinned
  revision also gives `revision`. A read by address gives its detail links instead.
- Items come in address order. `next_cursor` is the address of the last item; the next page
  starts after it, so a new object does not move the items of a later page.
- A list of an Inbox type (`inbox: true` in `joppa_tree`) keeps the Inbox order: unprocessed
  inputs first, then processed inputs, each group in address order. Its `next_cursor` is the
  Inbox cursor.
- No `revision`, `body`, `process` or `id`. Name them with `fields`.
- The next page: `{"workspace": "w", "parent": "requirement:r", "cursor": "ac:a3"}`.

With `related_to`, `note_status` or `fields`, the list keeps the earlier
filters and order. `total` counts every matching object, also on later pages. A list in address order gives the bare address of
its last item as `next_cursor`, so one sweep may mix plain pages and pages with `fields`.
A cursor that is not an address `kind:id` is refused: `invalid object cursor`.

## Named fields

Request:

```json
{"workspace": "w", "kind": "ac", "parent": "*",
 "fields": ["revision", "work_ready", "process.assessment"]}
```

Reply item: the address and each named path, also when its value is empty.

```json
{"address": "ac:a", "revision": 3, "work_ready": true, "process.assessment": null}
```

List fields: `address`, `data`, `kind`, `id`, `revision`, `parent`, `title`, `owner`,
`work_ready`, `at`, `body`, `signal`, `present`, `note_status`, `status`, `process`, `children`,
`child_counts`, `links`.

`child_counts` comes from the status projection of the last write. It has one entry for each
type of the present direct children that have a tone, in the order of the type index:
`{"kind": "ac", "total": 6, "passed": 4}`, where `passed` counts the green children. A node
without such a child has no `child_counts`. The board shows each entry in the row, such as
`4 / 6 ACs`.

Any other first part names a document field: `source_section.title` gives
`data.source_section.title`. A document field is a field of the object roles (such as
`text`, `tests` or `depends_on`), a `references` or `required_fields` key of the tree
schema, or a key of a returned document. Any other path is refused: `unknown field nope`.
The text of every type is `body`: `description`, `contract`, `data.description` and
`data.contract` are refused with `use body instead of description; read joppa_help`.

`status` is the tone, as in a plain list item; `note_status` gives the Inbox state.
`body` is the full text. `data` is the full stored `document.data`, and `data.<key>` is
one of its values. No field is cut. One requirement with all its criteria takes
two reads:

```json
{"workspace": "w", "address": "requirement:r"}
{"workspace": "w", "parent": "requirement:r",
 "fields": ["title", "status", "revision", "data.body"]}
```

## Node filters

`joppa_objects`, `joppa_search` and the HTTP routes `/objects` and `/search`
take the same filters (`src/node_filter.rs`). All filters must match.

| Filter | Keeps |
| --- | --- |
| `parent` | Each descendant of the node, at any depth. `*`: every level. |
| `depth` | Nodes at most N levels (1 to 100) below `parent`, or below the top without a parent address. `1`: the direct children, or the roots. |
| `kind` | Nodes of one type or of a list of types. |
| `status` | Nodes with one tone or a tone of a list: `red`, `purple`, `yellow`, `orange`, `grey`, `green`. The tone is the `status` of a list item. `purple` marks archived nodes: a list read with `purple` also keeps the current archived nodes, and the walk below `parent` also passes archived nodes. `joppa_search` reads them only with `all_versions`. |
| `owner` | Nodes whose `data.owner` is this member ID. |
| `assignee` | Nodes whose `data.assignee` is this member ID. Any node can have `data.assignee`. |
| `ready` | `false`: nodes that wait for a dependency, or for the answer that their readiness mark asks for (`assess` with `ready: false`, state `waiting`). A node of a type with the status rule `children` or `links` is also `false` when one of its counted children or linking objects is not ready. The counted ones are the same as for the status tone. `true`: every other node. |
| `data` | Nodes whose `document.data` contains this JSON object, as the JSONB operator `@>`. |

```json
{"workspace": "w", "parent": "domain:billing", "kind": ["ac"], "status": ["red", "grey"]}
{"workspace": "w", "data": {"tags": ["kyc"]}, "fields": ["title"]}
```

- Without `parent`, a filter reads every level. Without any filter, the list gives the roots.
- A list read keeps only present nodes, unless `status` names `purple`. Then
  `{"parent": "requirement:r", "status": "purple"}` gives the archived nodes below the requirement,
  `["green", "purple"]` gives the green present nodes and the archived nodes, and each archived
  item gives `present: false` when `fields` names it.
- HTTP takes `kind` and `status` comma-separated, `ready` as `true` or `false` and `data` as
  JSON text. A board link with these query keys, such as `/?workspace=w&assignee=user-1&ready=false`,
  opens the board with that list. The board pickers `owner`, `assignee`, `status` and `ready` show
  the link values, and a picker change writes its value to the link. The board filters and the
  board search read every level of the tree.
- A bad value is refused: `unknown status blue; use red, purple, yellow, orange, grey, green`,
  `status purple marks archived nodes; find them with joppa_objects, or with joppa_search and all_versions`
  (a search without `all_versions`),
  `depth must be an integer from 1 to 100`, `ready must be a boolean`, `data filter must be an object`.
- Indexes serve `owner`, `assignee`, the tone and `data` (migrations 039, 045 and 046).
- With `address` the filters are refused: `address cannot be combined with list filters`.
- A read with `work_filter` is refused: `work_filter was removed; use the node filters, for example kind and ready`.

## Reads by address

`docs/read-detail.md` gives the detail read, one revision, the revision headers, the read of
up to 10 addresses in one call, and `fields` with `address`.

## Tests

A test is a node (see [what green means](green.md)). Read the tests of a criterion and their
tones with `joppa_objects`, for example `{"workspace": "w", "parent": "ac:a", "kind": "test"}`,
and every red test with `{"workspace": "w", "kind": "test", "status": "red"}`. The tool
`joppa_tests` and the route `GET /api/workspaces/{w}/tests` were removed; a call of
`joppa_tests` is refused with `joppa_tests was removed; use joppa_objects with kind test`.

## Other read tools

Read Tasks with the node filters, for example `{"kind":"task","assignee":"alex","ready":true}`.
A call of `joppa_tasks` is refused with `joppa_tasks was removed; use joppa_objects with kind task, for example {"kind":"task","assignee":"alex","ready":true}`.

`joppa_timeline`, `joppa_history` and `joppa_search` lose the empty keys, `item_id` and the
echo of the request:

- `joppa_timeline` gives no `after` or `order`. A record of test results names itself only
  by its address `record/N`.
- `joppa_history` gives no `all`, `limit` or `signal`. Read the current status of an object
  with `joppa_objects`.
- A `joppa_search` result gives `address`, `title`, `revision`, `score` and `excerpt`, the
  first text of the object (`body` or `text`, at most 600 characters). It has no `kind`, `id` or `system_id`, no `req` on a requirement, and
  `workspace` only with `all_workspaces: true`. `historical`, `archived` and `exact_id`
  show only when they are `true`. The reply gives no `all_versions`, `all_workspaces`,
  `embedded`, `indexed`, `mode` or `semantic_status`, and `workspaces` only with
  `all_workspaces: true`. `total` counts every text match and
  the 100 nearest vector matches, so it can be above 100. When the query is an item ID, an
  address or a generated local ID of 8 characters `[a-z0-9]`, the reply gives only the
  exact matches, each with `exact_id: true`.

## Links of one object

`{"workspace": "w", "links_of": "domain:d", "direction": "incoming"}` gives the links of
`domain:d` in pages of 25 with `total` and `next_cursor`, in the order direction, relation,
address. `direction`, `relation` and `kind` are optional filters. An item names its
`direction`, `relation` and `kind` only when the request did not send that filter, and
its `relation` only when it is not `related`. `present: false` and `blocking: true` show
only when they apply. `next_cursor` is `direction/relation/address` of the last item.

## Writes

`joppa_command` and `joppa_batch` give `position`, `command` and `object`, without
`summary` or `system_ids`. When `object` gives `{address, revision}`, `command` gives only
its `op`. When the saved object has a parent, `object` also gives `parent_revision`, the
current revision of the parent. `replayed: true` marks a retry; a first write gives no `replayed`. A
`set_tree_schema` result gives `schema_revision`, the new schema revision, and its `command`
without the sent types. `joppa_attach` gives the `file`, and `joppa_attachments` the `files`. With `removed: true`,
`joppa_attachments` also gives the files detached in the last 14 days, after the current
files, newest detach first, each with `removed: true` and `removed_at`.

## `joppa_tree`

The tree gives `schema` with its `revision`; send it with `set_tree_schema`. There is no
`current_revision`, no write header (`actor`, `at`, `position`, `reason`) and no
`status_rules` copy: each type names its `status`. A type gives `inbox`, `root` and
`launch_blocker` only when they are `true`. `workspace_key` shows only when the request named
an alias of the workspace. `joppa_help` with a workspace and `section: "tree"` gives the same tree.

## Titles and texts

- `data.title`: one concise statement (max 128 characters by default). A type sets its own
  maximum as `title_max_length` in `joppa_tree`; `set_tree_schema` takes `title_max_length`.
- Each other text value: concise text (max `field_max_length` of the type, default 1024 characters). Over 1 kb: prefer attach as a file. Comments have no limit.
