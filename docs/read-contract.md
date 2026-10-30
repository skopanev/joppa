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
- No key repeats another key, the address or the request.
- Each list reply gives `workspace` and `position`, the journal position of its snapshot.
  Equal positions in several replies mean that no write came between them. A detail read
  gives neither: the request names the workspace.
- MCP replies give no `item_id`. To resolve a copied public ID, send it as `query` to
  `joppa_search`: the reply gives only that object, with `exact_id: true`.

## Limits for clients that send a larger `limit`

| Tool | `limit` above the page size | Paging |
| --- | --- | --- |
| `joppa_objects` lists, also with `fields`, `work_filter`, `related_to`, `note_status` | clamped to 25, never refused | `next_cursor` and `total` |
| `joppa_objects` with `links_of` | clamped to 25 | `next_cursor` and `total` |
| `joppa_objects` with `history: true` | fixed pages of 25 | `next_cursor` and `total` |
| `joppa_objects` detail comments | fixed pages of 25 | `comments_cursor` |
| `joppa_tasks`, `joppa_timeline`, `joppa_history` | clamped to 25, default 25 | `next_cursor` and `total` |
| `joppa_search` | clamped to 25, default 10 | `next_cursor` and `total` |
| `joppa_list` | fixed pages of 25 | `next_cursor` and `total` |
| `joppa_tests` overview (no `ac`, `parent`, `kind` or `task`) | fixed pages of 25 | `next_cursor` and `total` |
| `joppa_tests` with `ac` | more than 100 criteria are refused | none |
| `joppa_tests` with `parent` or `kind` | 100 criteria a page | `next_cursor` and `total` |

Every MCP list reply is `{items, total, next_cursor}`. `total` counts every matching entry,
also on later pages. Send `next_cursor` back as `cursor`; a missing `next_cursor` means the
last page. A client that sends `limit: 100` gets at most 25 items and must follow `next_cursor`.

## List mode: `joppa_objects` without `address`

Request:

```json
{"workspace": "w", "parent": "requirement:r", "limit": 100}
```

`limit` is capped at 25. `parent` is a node address, `*` for every node, or absent for the
roots. `kind` filters by type. Reply:

```json
{
  "workspace": "w",
  "position": 412,
  "total": 27,
  "next_cursor": "ac:a3",
  "items": [
    {"address": "ac:a", "title": "Each saved task has an owner", "status": "green", "children": 2},
    {"address": "ac:a1", "title": "A refused save stores nothing", "status": "grey"}
  ]
}
```

- `status` is the status tone. `children` is the number of present children, only when above 0.
- Items come in address order. `next_cursor` is the address of the last item; the next page
  starts after it, so a new object does not move the items of a later page.
- A list of an Inbox type (`inbox: true` in `joppa_tree`) keeps the Inbox order: unprocessed
  inputs first, then processed inputs, each group in address order. Its `next_cursor` is the
  Inbox cursor.
- No `revision`, `description`, `process` or `id`. Name them with `fields`.
- The next page: `{"workspace": "w", "parent": "requirement:r", "cursor": "ac:a3"}`.

With `work_filter`, `related_to`, `note_status` or `fields`, the list keeps the earlier
filters and order. `total` counts every matching object, also on later pages. A list in address order gives the bare address of
its last item as `next_cursor`, so one sweep may mix plain pages and pages with `fields`.
A cursor that is not an address `kind:id` is refused: `invalid object cursor`.

## Named fields

Request:

```json
{"workspace": "w", "kind": "ac", "parent": "*", "work_filter": "unstarted",
 "fields": ["revision", "contract_revision", "work_ready", "review_required",
            "process.planning_claim", "work_summary"]}
```

Reply item: the address and each named path, also when its value is empty.

```json
{"address": "ac:a", "revision": 3, "contract_revision": 2, "work_ready": true,
 "review_required": false, "process.planning_claim": null,
 "work_summary": {"active": 0, "done": 1, "retired": 0}}
```

A path whose first part is not a list field is refused: `unknown field nope`.
List fields: `address`, `data`, `kind`, `id`, `revision`, `contract_revision`, `parent`,
`title`, `owner`, `review_required`, `work_ready`, `at`, `description`, `signal`,
`present`, `note_status`, `status`, `process`, `work_summary`.

`status` is the tone, as in a plain list item; `note_status` gives the Inbox state.
`description` is a preview of 300 characters. `data` is the full stored `document.data`, and
`data.<key>` is one of its values, never cut. One requirement with all its criteria takes
two reads:

```json
{"workspace": "w", "address": "requirement:r"}
{"workspace": "w", "parent": "requirement:r",
 "fields": ["title", "status", "revision", "data.description", "data.tests"]}
```

## Detail mode: `joppa_objects` with `address`

Request: `{"workspace": "w", "address": "task:t"}`. Reply (empty keys omitted):

```json
{
  "revision": 3,
  "contract_revision": 2,
  "parent": "ac:a",
  "data": {"title": "...", "owner": "owner", "contract": "...", "note": null, "projects": []},
  "status": {"label": "In progress", "tone": "yellow"},
  "process": {"status": "in_progress", "holder": "bot"},
  "comments": {
    "total": 30,
    "next_cursor": "25",
    "items": [{"id": "c00", "text": "Finding 1", "actor": "bot", "actor_type": "agent",
               "at": "...", "withdrawn": false}]
  },
  "links": [
    {"target": "ac:b", "title": "..."},
    {"target": "task:u", "relation": "depends_on", "title": "...", "revision": 2,
     "current_revision": 4, "blocking": true}],
  "link_groups": [{"direction": "incoming", "relation": "related", "kind": "task", "count": 2}]
}
```

- `revision` is the current revision. Send it with `save_object`.
- `parent` and `data` are the stored values, byte for byte. A root object has no `parent`.
- `contract_revision` shows only when it differs from `revision`.
- There are no `workspace`, `workspace_key`, `address`, `position`, `document`, `kind`,
  `id`, `current_revision`, `revision_count`, `historical` or `metadata_scope` keys. The
  actor, time and reason of a revision come with `history: true` or `revision: N`.
- `present: false` marks an archived object. A present object has no `present` key.
- `links` is the one list of all outgoing links, without pages: `target` and the `title`
  of the target. `relation` shows only when it is not `related`. A pinned link adds its
  `revision` pin and the `current_revision` of the target. A blocking link adds
  `blocking: true`. `present: false` marks an archived target.
- `save_object` takes the link items back as they are. It ignores `title`,
  `current_revision` and `present`, and a link without `relation` is `related`.
- `comments` are in time order. A comment has no `author` or `created_at` when they repeat
  `actor` and `at`.
- `link_groups` counts the incoming links by relation and kind.
- `references` gives the title of each object that a data field names.
- An object without comments has no `comments` key. An object without outgoing links has no
  `links` key.
- `files` lists the attachments. `annotations` has no `attachments` copy.
- `process` keeps one `assessment`; `readiness_blocker` is omitted when it repeats it.
  `process` and `assessment` give no copy of the address, of `req` or of its revision.
  The `req` of a criterion is its `parent`, so `process` of a criterion has no `req`.
  Test results decide a criterion, so its `process` has no run `status` or `revision`: the
  requirement revision that `assess` sends is in `assessment`. `verification` shows only the
  `pass` or `fail` of a finished run, on every kind of object.
- `process.revision` of a requirement shows only when it differs from `revision`.
  `process.status` of a Note shows only when it differs from `data.status`.
  `process.confirmed` shows only when it is `true`.
- `work_ready` shows only when it is `false`. `review_required` shows only when it is `true`.
- `verification_summary` gives only the counts above 0. Read a missing count as 0.
- An object without remote links has no `remote_links` or `remote_review_required`. Read
  them as `[]` and `false`. Its `remote_links_revision` shows only when it is above 0: send
  a missing one as 0 with `set_remote_links`.
- Next comment page: `{"workspace": "w", "address": "task:t", "comments_cursor": "25"}`.
- `comments_cursor` is an offset. A comment added between two reads moves the later items
  by one place. Read the first page again for a stable view.
- `links_of` reads the incoming links in pages of 25.

## One revision and the revision headers

`{"workspace": "w", "address": "task:t", "revision": 1}` gives revision 1 with
`historical` and `current_revision`, without empty keys outside `document`. It follows the
detail rules for copies of the address and for an empty remote link list.

`{"workspace": "w", "address": "task:t", "history": true}` gives the revision headers,
oldest first, in pages of 25. Send `next_cursor` back as `cursor` for the next page:

```json
{"workspace": "w", "position": 412, "address": "task:t", "current_revision": 3, "total": 3,
 "items": [{"revision": 1, "at": "...", "actor": "bot", "reason": "Create"},
           {"revision": 2, "at": "...", "actor": "bot", "reason": "Sharper contract"},
           {"revision": 3, "at": "...", "actor": "bot", "reason": "Claim"}]}
```

## `joppa_tests`

- `{"workspace": "w"}`: the overview of green and red criteria in pages of 25, with
  `workspace` and `position`.
- `{"workspace": "w", "ac": "ac:a"}`: one criterion, as before.
- `{"workspace": "w", "ac": "ac:a,ac:b"}`: `{"items": [...]}`. At most 100 criteria in one call;
  101 are refused with `at most 100 criteria in one call`.
- `{"workspace": "w", "kind": "ac", "parent": "*"}`: every criterion in address order, 100 a
  page: `{"total": 212, "next_cursor": "100", "items": [...]}`. Send `cursor` for the next page.
  `parent` gives the direct children of one node.
  With 101 criteria, the first page has 100 items and `next_cursor` `"100"`; the second has 1.
- A missing `tests` key means no tests. A missing `message` means no failure message.

## Other read tools

`joppa_tasks`, `joppa_timeline`, `joppa_history` and `joppa_search` lose the empty keys,
`item_id` and the echo of the request:

- `joppa_tasks` gives its tasks as `items`, without `view`. A task has no `id` (the address
  is the ID) and no `ac` when the criterion is its `parent`. `readiness_blocker` is omitted
  when it repeats `assessment`. `signal` gives `tone`, `label` and `at` only. A done or
  retired task has no `waiting_reason`.
- `joppa_timeline` gives no `after` or `order`. A record of test results names itself only
  by its address `record/N`.
- `joppa_history` gives `workspace` and `position`, and no `all`, `limit` or `signal`. Read
  the current status of an object with `joppa_objects`.
- A `joppa_search` result has no `system_id`, no `req` on a requirement, and `workspace`
  only with `all_workspaces: true`. The reply gives no `all_versions` or `all_workspaces`,
  and `workspaces` only with `all_workspaces: true`. `total` counts every text match and
  the 100 nearest vector matches, so it can be above 100. When the query is an item ID or
  an address, the reply gives only the exact matches, each with `exact_id: true`.

## Links of one object

`{"workspace": "w", "links_of": "domain:d", "direction": "incoming"}` gives the links of
`domain:d` in pages of 25 with `workspace`, `position`, `total` and `next_cursor`, in the
order direction, relation, address. `direction`, `relation` and `kind` are optional
filters. An item names its `direction`, `relation` and `kind` only when the request did not
send that filter. `next_cursor` is `direction/relation/address` of the last item.

## Writes

`joppa_command` and `joppa_batch` give `position`, `command`, `object` and `replayed`, without
`summary` or `system_ids`. A `set_tree_schema` result gives `schema_revision`, the new schema
revision, and its `command` without the sent types.

## Titles and texts

- `data.title`: one concise statement (max 128 characters by default). A type sets its own
  maximum as `title_max_length` in `joppa_tree`; `set_tree_schema` takes `title_max_length`
  and the older `title_preview_length`.
- Each other text value: concise text (max `field_max_length` of the type, default 1024 characters). Over 1 kb: prefer attach as a file. Comments have no limit.
