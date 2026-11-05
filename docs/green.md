# What green means

## Tests are nodes

A test is a node of a type with role `test` and status rule `own`. The default type is `test`;
its parents are the criterion types. One node names one test in `data.title`:

```json
{"op": "save_object", "kind": "test", "revision": 0, "parent": "ac:refund",
 "data": {"title": "tests/billing.rs > refund_keeps_access", "owner": "alex"}, "reason": "Bind"}
```

The block `test_results` of the workspace schema gives the form of a test name. See
[the workspace schema](schema.md) for each rule of the block. A criterion has at most
`max_tests` test nodes, and it names each test once: `test "<name>" is listed twice`.
A test that proves two criteria has one node under each. Read the tests of a criterion with
`joppa_objects {"parent": "ac:refund", "kind": "test"}`, and the red tests with
`{"kind": "test", "status": "red"}`.

## No AC, no test

Every test in the repository must have a present test node.

- Never add or rename a test without its test node in the same change.
- Archive the test node of a deleted test. Archive the test nodes of a criterion before the criterion.
- `joppa_record_results` gives `unbound`: the test names of the record that are the title of no
  present test node in the workspace. `unbound` does not check under which criterion a node stands.
- A full-run record must give `"unbound": []` before a merge.

## Records

Send each test run with `joppa_record_results {workspace, commit, results}`.
Each result gives `test` and `status`, and it can give the fields that the schema lists.
The server records the actor and the time. A caller cannot set them.
A record changes only the tests that it names. A save of a document cannot set a result.

## The tone of a test node

The newest result of the test after the last reset of its node decides:

- Red: an outcome in `red_if_any`.
- Green: an outcome in `green_if_all`.
- Grey: no result, or a `skip` or `todo` result.

With `same_commit: "fail_wins"`, a failure in any record of the newest commit decides the test.
An older skip or todo of that commit never hides a newer result.
A newer commit that passes the test clears an older failure.
Time alone never changes a tone.

## One rollup on every level

A node with `status: "children"` takes the highest tone of its present counted children, and
a node with `status: "links"` the highest tone of the present objects that link to it, in the
order red, orange, yellow, grey, green.

- A node with no counted child or link is orange: a criterion without a test node is orange.
- A child with `status: "none"`, for example a Task, does not count.
- A node that waits is at least yellow: it waits for a dependency or for the answer to its
  question (`assess` with `ready: false`).
- An archived node is purple and does not count.

## Changes

With `reset_on_change`, a change of a node or of a node above it makes the earlier records stop
counting for the test nodes at and below it. Such a test is grey until a newer record arrives.

- A new title, text, parent or non-blocking link of a node resets the test nodes at and below it.
- An archive resets only the test nodes at and below the archived object.
- A write of a Task or of an Inbox item and a blocking `depends_on` link reset nothing.
- A change of a criterion or a test node removes its own readiness mark. Each node below keeps its mark.

Old records stay in the history. They are facts about the old text.

## Acceptance

`accept_commit {target, revision, commit}` records that a person accepts a node at one commit.
Only the `data.owner` of the node accepts. An agent cannot accept a node.

- The commit needs a record for every present test node at and below the node.
- Every recorded case of these tests has an outcome in `green_if_all`.
- With `reset_on_change`, only the records after the last reset of each test node count.

Else the server refuses: `acceptance needs records on the commit where every test below passes`.
A Task does not make a criterion green.

## Limits

A passing test can check the wrong thing or contain an empty assertion.
A record cannot show that a test is sufficient. A person must review the tests.
The criterion set can omit a significant condition. Acceptance does not prove that the set is complete.
