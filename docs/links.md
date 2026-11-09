# Relations and other workspaces

## Relations in one workspace

A node holds `relations`: one key for each relation of the registry, with an optional `title`
of the node for that relation and its `edges`. An edge is `{"to", "when"?, "revision"?}`:
the target address or public item ID, the condition of a flow step, and the pinned target
revision. A relation does not change the parent of a node.

```json
{"relations": {
  "depends_on": {"edges": [{"to": "ac:ledger-table"}]},
  "related": {"edges": [{"to": "source:limits", "revision": 3}]},
  "swap": {"title": "Enter amount", "edges": [{"to": "ac:pin-screen", "when": "limit ok"}]}}}
```

- `depends_on` makes the node wait until the target is green. Cycles are refused, also through
  inherited parent dependencies. A new revision of the target asks for no review.
- `related` records a reference between nodes. It blocks nothing, and cycles are permitted.
- Every other key is a flow, such as a sequence of client screens. A flow allows cycles,
  blocks nothing, resets no criterion and counts for no status rule.

A live depends_on target is satisfied when it is a green criterion, or when it has live
criteria below it and each is green. An archived target does not make work wait. The status
rule `links`, a typed field with `from: links` and the source reset count depends_on and
related edges, never a flow. `related_to` lists the depends_on and related neighbours.

A save merges `relations` by key: a sent key replaces its `title` and `edges`, `null` removes
the key, and the other keys stay. A key with `edges: []` stays and counts as used. A node holds
at most 64 keys, a key at most 200 edges, and a `title` or a `when` at most 200 characters.
A key that the registry does not have is refused: `unknown link relation`. The removed field
`links` is refused: `links was removed; use relations`.

### The registry

`joppa_tree` gives `schema.relations`: each key with `title`, `description` and `system`.
Every workspace has the system relations `depends_on` and `related`. Change the registry with
`set_tree_schema`:

```json
{"op": "set_tree_schema", "revision": 7, "reason": "Model the swap flow",
 "put_relations": {"swap": {"title": "Swap", "description": "Client screens"}}}
```

- `put_relations` adds a relation, or replaces its `title` and `description`. A key obeys the
  ID rule. Only the server sets `system`.
- `remove_relations` removes relations. A system relation and a relation of a live node are
  refused. `restore_object` refuses a node with a removed relation: `declare relation <key> first`.
- A command without them keeps the registry.

### Reads

One object read gives `relations` with titled edges and `relation_groups`: a count for each
relation and kind of the incoming edges. Read the edges in pages of 25 with `joppa_objects`:

```json
{"workspace": "product", "relations_of": "domain:billing", "direction": "incoming", "relation": "related", "kind": "note"}
```

`direction`, `relation` and `kind` are optional filters. The reply gives `items` in the order
direction, relation, address, and `next_cursor`. Send `cursor` with that value for the next
page. `next_cursor` is null on the last page. An item gives `address`, `title`, `kind`,
`relation`, `direction`, `when` and `revision` when set, and `present`. `revision` is the
target revision that the edge pins. The removed parameter `links_of` is refused:
`links_of was removed; use relations_of`.

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
- A blocking link makes the work below the object wait until the target is ready: its pin is current, every criterion at and below it is green, and its own blocking links are ready.
- Without read access, a status has `access: false` and null values, and a blocking link counts as not ready.
- An empty `links` list removes the remote links of the object.

`workspace_key` in joppa_tree is the immutable ID of the workspace. joppa_tree gives it only
when the request named an alias of the workspace.
