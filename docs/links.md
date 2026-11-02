# Links and other workspaces

## Links in one workspace

An object can link to many objects of any type in the same workspace.
A link does not change the parent of an object.

- `related` records context. Cycles are permitted.
- `depends_on` records a dependency on a node. A new revision of the target asks for no review.
- `blocking: true` makes the dependency a condition for the start of work.

A blocking target needs completion evidence from its role: a verified criterion, a requirement
whose current checks pass, or a done Task. Blocking cycles, also through inherited parent
dependencies, are refused. A blocking dependency prevents claims until its target is ready.

One object read gives `links`, its outgoing links with titles, and `link_groups`: a count for
each direction, relation and kind of the other object. The MCP read keeps only the incoming
counts. Read the links in pages with `joppa_objects`:

```json
{"workspace": "product", "links_of": "domain:billing", "direction": "incoming", "relation": "related", "kind": "note", "limit": 50}
```

`direction`, `relation` and `kind` are optional filters. The reply gives `items` in the order
direction, relation, address, and `next_cursor`. Send `cursor` with that value for the next
page. `next_cursor` is null on the last page. In an item, `address`, `title`, `kind` and
`present` describe the other object now. `revision` is the target revision that the link pins,
or null. Read the other object to get its current revision.

## Other workspaces

One product can use several workspaces, for example one for the product and one for each
component. Each workspace keeps its own tree, search, history and Tasks.

A node stands for another workspace when its data has `workspace` with the ID of that
workspace. The server refuses the own workspace, a name in place of the ID, and a workspace
that the writer cannot read. A read of the node gives `child_workspace`: the status and the
green, red and total counts of the criteria of that workspace. A reader without access sees
only `access: false`. This status is for display. It is not evidence for a local criterion.

`set_remote_links {target, revision, links, reason}` replaces the links of one object to
objects of other workspaces. `revision` is the `remote_links_revision` that you read, 0 when
the object has none. Each link has `workspace` (name or ID; the server stores the ID),
`target` (kind:id), `relation`, `revision` and `blocking`. A blocking link needs `revision`:

```json
{"op": "set_remote_links", "target": "ac:checkout", "revision": 0, "reason": "Checkout uses the auth API",
 "links": [{"workspace": "backend", "target": "requirement:auth-api", "relation": "depends_on", "revision": 3, "blocking": true}]}
```

- The server refuses an absent or archived target, a revision above the current one, and a workspace that the writer cannot read.
- A blocking link that closes a dependency cycle across workspaces is refused.
- A read gives `remote_links`, each with `status`: `access`, `present`, `current_revision`, `review_required` and `satisfied`.
- `review_required` is true when the target has a newer revision, or is archived or absent. The object then has `remote_review_required: true`.
- A new, changed or removed pin resets the criteria below the object. Save the links again with the new revision after the review.
- A plan or a Task claimed before such a change cannot finish on the old pin: finish_planning and complete_task refuse with "the remote links changed after the claim; release and claim again".
- A blocking link makes the work below the object wait until the target is ready: its pin is current, every criterion at and below it is green, and its own blocking links are ready.
- Without read access, a status has `access: false` and null values, and a blocking link counts as not ready.
- An empty `links` list removes the remote links of the object.

Claims, completions and acceptances check this wait with the access of the writer. They lock
every workspace that a chain of blocking remote links reaches, in sorted ID order. A write that
reaches a workspace outside its locks fails and asks for a retry.
`workspace_key` in joppa_tree is the immutable ID of the workspace. joppa_tree gives it only
when the request named an alias of the workspace.
