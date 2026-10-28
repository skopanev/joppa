# MCP read contract

This file gives the exact shape of the MCP reads. It matches `requirement:read-contract`.
The compact shape is the default. Send `"verbose": true` on any read tool to get the shape
of the earlier release.

## Common rules

- A page holds at most 25 items. A missing `next_cursor` means the last page.
- Send the `next_cursor` value back as `cursor` (lists), `comments_cursor` or `links_cursor`.
- Keys whose value is `null`, `""`, `[]` or `{}` are omitted, except inside `document`,
  `data`, `command` and `remote_links`. Those stay byte for byte as stored. `false` and `0` stay.
- A top-level `items: []` stays when a list is empty.
- Read an omitted key as empty: no value, no list, no map.

## Limits for clients that send a larger `limit`

| Tool | `limit` above the page size | Paging |
| --- | --- | --- |
| `joppa_objects` lists, also with `fields`, `work_filter`, `related_to`, `note_status` | clamped to 25, never refused | `next_cursor`; `total` on plain lists |
| `joppa_objects` with `links_of` | clamped to 25 | `next_cursor` |
| `joppa_objects` detail comments and links | fixed pages of 25 | `comments_cursor`, `links_cursor` |
| `joppa_tasks` | unchanged: up to 100 | `next_cursor` as before |
| `joppa_tests` with `ac` | more than 100 criteria are refused | none |
| `joppa_tests` with `parent` or `kind` | 100 criteria a page | `next_cursor` and `total` |

A client that sends `limit: 100` to `joppa_objects` gets 25 items and must follow `next_cursor`.

## List mode: `joppa_objects` without `address`

Request:

```json
{"workspace": "w", "parent": "requirement:r", "limit": 100}
```

`limit` is capped at 25. `parent` is a node address, `*` for every node, or absent for the
roots. `kind` filters by type. Reply:

```json
{
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
- No `revision`, `description`, `process`, `id` or `item_id`.
- The next page: `{"workspace": "w", "parent": "requirement:r", "cursor": "ac:a3"}`.

With `work_filter`, `related_to`, `note_status` or `fields`, the list keeps the earlier
filters and order. `total` is then absent.

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
List fields: `address`, `kind`, `id`, `revision`, `contract_revision`, `parent`, `title`,
`owner`, `review_required`, `work_ready`, `item_id`, `at`, `description`, `signal`,
`present`, `note_status`, `status`, `process`, `work_summary`.

## Detail mode: `joppa_objects` with `address`

Request: `{"workspace": "w", "address": "task:t"}`. Reply (empty keys omitted):

```json
{
  "address": "task:t",
  "kind": "task",
  "item_id": "w-T-AB12CD34EF",
  "revision": 3,
  "current_revision": 3,
  "contract_revision": 2,
  "document": {"revision": 3, "actor": "bot", "at": "...", "reason": "...",
               "parent": "ac:a", "present": true,
               "data": {"title": "...", "owner": "owner", "contract": "...", "note": null,
                        "projects": []},
               "links": [{"target": "ac:b", "relation": "related", "blocking": false, "revision": null}]},
  "status": {"label": "In progress", "tone": "yellow"},
  "process": {"status": "in_progress", "holder": "bot"},
  "work_ready": true,
  "review_required": false,
  "revisions": [
    {"revision": 1, "at": "...", "actor": "bot", "reason": "Create"},
    {"revision": 2, "at": "...", "actor": "bot", "reason": "Sharper contract"},
    {"revision": 3, "at": "...", "actor": "bot", "reason": "Claim"}
  ],
  "comments": {
    "total": 30,
    "next_cursor": "25",
    "items": [{"id": "c00", "text": "Finding 1", "actor": "bot", "actor_type": "agent",
               "at": "...", "withdrawn": false}]
  },
  "links": {"total": 1, "items": [{"target": "ac:b", "relation": "related", "title": "...", "present": true}]},
  "files": [],
  "references": []
}
```

- `document` is the stored revision, unchanged. `document.links` is complete.
- `comments` are in time order. A comment has no `author` or `created_at` when they repeat
  `actor` and `at`.
- `links` is the titled list of outgoing links; `link_groups` stays for incoming counts.
- `process` keeps one `assessment`; `readiness_blocker` is omitted when it repeats it.
- Next comment page: `{"workspace": "w", "address": "task:t", "comments_cursor": "25"}`.
- Next link page: `{"workspace": "w", "address": "task:t", "links_cursor": "25"}`.
- `comments_cursor` and `links_cursor` are offsets. A comment or link added between two
  reads moves the later items by one place. Read the first page again for a stable view.

## One revision

`{"workspace": "w", "address": "task:t", "revision": 1}` gives revision 1 as before, without
empty keys outside `document`. `"history": true` gives the revision counts.

## `joppa_tests`

- `{"workspace": "w", "ac": "ac:a"}`: one criterion, as before.
- `{"workspace": "w", "ac": "ac:a,ac:b"}`: `{"items": [...]}`. At most 100 criteria in one call;
  101 are refused with `at most 100 criteria in one call`.
- `{"workspace": "w", "kind": "ac", "parent": "*"}`: every criterion in address order, 100 a
  page: `{"total": 212, "next_cursor": "100", "items": [...]}`. Send `cursor` for the next page.
  `parent` gives the direct children of one node.
  With 101 criteria, the first page has 100 items and `next_cursor` `"100"`; the second has 1.
- A missing `tests` key means no tests. A missing `message` means no failure message.

## Other read tools

`joppa_read`, `joppa_tasks`, `joppa_timeline`, `joppa_history` and `joppa_search` keep their
fields and paging. Only the empty keys go. `joppa_tasks` keeps `limit` up to 100 and
`next_cursor`.

## Titles and texts

- `data.title`: one concise statement (max 128 characters by default). A type sets its own
  maximum as `title_max_length` in `joppa_tree`; `set_tree_schema` takes `title_max_length`
  and the older `title_preview_length`.
- Each other text value: concise text (max 1024 characters). Comments have no limit.
