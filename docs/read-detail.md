# MCP reads by address

This file gives the exact shape of the `joppa_objects` reads by `address`. It matches
`requirement:read-contract`. `docs/read-contract.md` gives the common rules and the lists.

## Detail mode: `joppa_objects` with `address`

Request: `{"workspace": "w", "address": "task:t"}`. Reply (empty keys omitted):

```json
{
  "revision": 3,
  "parent": "ac:a",
  "data": {"title": "...", "owner": "owner", "body": "...", "note": null, "projects": []},
  "status": {"label": "Waits for a dependency", "tone": "yellow"},
  "work_ready": false,
  "comments": {
    "total": 30,
    "next_cursor": "25",
    "items": [{"id": "c00", "text": "Finding 1", "actor": "bot",
               "at": "...", "withdrawn": false}]
  },
  "relations": {
    "related": {"edges": [{"to": "ac:b", "title": "..."}]},
    "depends_on": {"edges": [{"to": "task:u", "title": "...", "revision": 2,
                              "current_revision": 4}]}},
  "relation_groups": [{"direction": "incoming", "relation": "related", "kind": "task", "count": 2}]
}
```

- `revision` is the current revision. Send it with `save_object`.
- `parent` and `data` are the stored values, byte for byte. A root object has no `parent`.
- There are no `workspace`, `workspace_key`, `address`, `position`, `document`, `kind`,
  `id`, `current_revision`, `revision_count`, `historical` or `metadata_scope` keys. The
  actor, time and reason of a revision come with `history: true` or `revision: N`.
- `present: false` marks an archived object. A present object has no `present` key.
- `relations` gives every key with its `title` and all its edges, without pages: `to`, `when`
  and the `title` of the target. A pinned edge adds its `revision` pin and the
  `current_revision` of the target. `present: false` marks an archived target. A key without
  edges keeps `edges: []`.
- `save_object` takes the relations back as they are. It ignores `title` of an edge,
  `current_revision` and `present`.
- `comments` are in time order. A comment has no `author` or `created_at` when they repeat
  `actor` and `at`.
- `relation_groups` counts the incoming edges by relation and kind.
- `references` gives the title of each object that a data field names.
- An object without comments has no `comments` key. An object without relations gives
  `relations: {}`.
- `files` lists the attachments. `annotations` has no `attachments` copy.
- `process` is the same for every type: `annotation_address`, `verification` and the `status`
  of an inbox type. `annotation_address` shows only when it
  differs from the address.
- `verification` shows only the `pass` or `fail` of a finished run.
- `process.status` of a Note shows only when it differs from `data.status`.
- `work_ready` shows only when it is `false`.
- An object without remote links has no `remote_links` or `remote_review_required`. Read
  them as `[]` and `false`. Its `remote_links_revision` shows only when it is above 0: send
  a missing one as 0 with `set_remote_links`.
- Next comment page: `{"workspace": "w", "address": "task:t", "comments_cursor": "25"}`.
- `comments_cursor` is an offset. A comment added between two reads moves the later items
  by one place. Read the first page again for a stable view.
- `relations_of` reads the edges in both directions in pages of 25.

## One revision and the revision headers

`{"workspace": "w", "address": "task:t", "revision": 1}` gives revision 1 as `revision`,
`parent`, `data` and `relations`, with `current_revision`. A revision stored before the
relations gives its links as relations by the replay rule. `present: false` marks an archived
revision. It gives no `document`, `historical`, actor, time, reason, status, process, comments
or link counts.

`{"workspace": "w", "address": "task:t", "history": true}` gives the revision headers,
oldest first, in pages of 25. Send `next_cursor` back as `cursor` for the next page:

```json
{"current_revision": 3, "total": 3,
 "items": [{"revision": 1, "at": "...", "actor": "bot", "reason": "Create"},
           {"revision": 2, "at": "...", "actor": "bot", "reason": "Sharper contract"},
           {"revision": 3, "at": "...", "actor": "bot", "reason": "Assign the work"}]}
```

## More addresses in one read

`address` takes 1 to 25 addresses separated by commas, as many as one page. Spaces around
an address are ignored.

```json
{"workspace": "w", "address": "ac:a,requirement:r"}
```

One address gives the object itself, as above. 2 to 25 addresses give `{"items": [...]}`
in the order of the request. Each item is the detail of one object with its `address`:

```json
{"items": [{"address": "ac:a", "revision": 2, "parent": "requirement:r", "data": {}},
           {"address": "requirement:r", "revision": 5, "data": {}}]}
```

- More than 25 addresses are refused: `at most 25 addresses in one call`.
- An empty address is refused: `empty address in the address list`.
- `history`, `revision` and `comments_cursor` need one address. With more addresses they are
  refused: `history, revision and comments_cursor read one address`.
- An unknown address refuses the whole read: `unknown object`.
- A reply of 2 or more objects above 256 KB is refused. A read of one object comes in full.

## Named fields of an object read by its address

`fields` with `address` gives only the named paths. `address` shows only for 2 to 25
addresses, or when the request named an alias. Name `revision` to get the revision.

```json
{"workspace": "w", "address": "ac:a,requirement:r", "fields": ["title", "status"]}
```

```json
{"items": [{"address": "ac:a", "title": "A", "status": "green"},
           {"address": "requirement:r", "title": "R", "status": "grey"}]}
```

- A list field has the value of the list item: `status` is the tone, `status.label` gives
  the label, and `body` is the full text, the same as `data.body`.
  `docs/read-contract.md` names the list fields.
- A detail key gives its value in the detail read: `relations`, `comments`, `files`,
  `references`, `relation_groups`, `remote_links`, `remote_review_required`,
  `remote_links_revision`, `child_workspace`, `annotations` and
  `result`. A key that the detail omits gives `null`.
- Any other first part names a document field: `note` gives `data.note`. The rules of
  `docs/read-contract.md` refuse an unknown path and the old text fields `description` and
  `contract`.
- `fields` reads the current revision, or the revision that `revision` names. Then the
  document fields and `revision` come from that revision, and the state fields (`status`,
  `signal`, `work_ready`, `process`, `children`) stay current. With
  `history` it is refused: `fields reads one revision; history gives the revision headers`.
