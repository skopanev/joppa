# What green means

## Tests are nodes

A test is a node of a type with role `test` and status rule `own`. The default type is `test`.
Its parents are the criterion types. No rule checks the form of the title. The schema gives the
fields of the type. The recommended recipe uses these fields:

- `title`: the scenario in business language, with concrete numbers and no code.
- `body`: the inputs and the expected outcome.
- `case`: `nominal`, `boundary` or `off_nominal`.
  - `nominal`: the normal working path.
  - `boundary`: the edges of the limits.
  - `off_nominal`: errors and safe refusals.
- `command`: the exact isolated command that runs this one test. A manual check is no test
  node: see [Manual checks](#manual-checks).

```json
{"op": "save_object", "kind": "test", "revision": 0, "parent": "ac:refund",
 "data": {"title": "A refund of 50 EUR over the limit of 40 EUR is refused", "owner": "alex",
  "body": "Inputs: a paid order of 120 EUR and a refund limit of 40 EUR. Expected: a refund of 50 EUR is refused, and the order keeps its access.",
  "case": "boundary", "command": "cargo test --test refunds refund_over_the_limit_is_refused"},
 "reason": "Prove the refund limit"}
```

A criterion has at most `max_tests` test nodes. A test that proves two criteria has one node
under each. Read the tests of a criterion with
`joppa_objects {"parent": "ac:refund", "kind": "test"}`, and the red tests with
`{"kind": "test", "status": "red"}`.

## Test-5 checklist

Check each test node against five rules:

1. Business language and concrete numbers. No code in the title.
2. One test, one diagnosis.
3. The normal path, the boundaries and a safe failure.
4. The real system. No mocks of the logic under test.
5. A fast, isolated command.

All tests under one AC together cover 100% of its conditions. The AC is green only when all of
them are green at the same time.

## No AC, no test

Every test in the repository must have a present test node.

- Never add or rename a test without its test node in the same change.
- Archive the test node of a deleted test. Archive the test nodes of a criterion before the criterion.
- `joppa_record_results` gives `unbound`: the tests of the record that bind to no present test
  node. `unbound` does not check under which criterion a node stands.
- A full-run record must give `"unbound": []` before a merge.

## Records

Send each test run with `joppa_record_results {workspace, commit, results}`.
Each result gives `test` and `status`, and it can give the fields that the schema lists.

- Name each test by the address of its node, such as `test:<id>`, or by its public item ID.
  It binds that one test node.
- A title binds every present test node with that title. A test that proves two criteria has
  one node under each, and one result with its title counts for each of them.
- A name with no present test node goes to `unbound`, and the call does not fail. The server
  keeps the result under the given name. A test node that gets that title later counts it.

The server stores a bound result under the address of each of its test nodes. A renamed node
keeps its results, and a new title resets nothing. The node keeps its earlier title as a name,
so a record by that title still binds it.
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
a node with `status: "links"` the highest tone of the present objects that link to it by a
depends_on or related edge or a reference field, in the order red, orange, yellow, grey, green.
A flow edge does not count.

- A node with no counted child or link is orange: a criterion without a test node is orange.
- A child with `status: "none"`, for example a Task, does not count.
- A node that waits for a depends_on target is at least yellow.
- An archived node is purple and does not count.

## Changes

With `reset_on_change`, a change of a node or of a node above it makes the earlier records stop
counting for the test nodes at and below it. Such a test is grey until a newer record arrives.

- Any new parent, depends_on or related edge, or data value of a node resets the test nodes at
  and below it. This holds for each node type, a new pin included. A flow edge resets nothing.
- A new title of a test node resets nothing. The title states the scenario, and the results
  stay with the node.
- A revision with the same parent, depends_on and related edges and data resets nothing. An example is the
  specification revision that a criterion save gives.
- An archive resets only the test nodes at and below the archived object.
- A node of a type with `source: true` that gets a new title, body or file resets each present
  criterion that names it in a reference field or that has a depends_on or related edge with it,
  in either direction. A flow edge resets nothing.
  A detach and a new value of another field reset nothing.

Old records stay in the history. They are facts about the old text.

## Manual checks

A person proves a criterion without a fake test: `joppa_attach` (or the `attach` op) on the AC
with `check_result` `pass` or `fail`, and optionally `valid_until` (`YYYY-MM-DD`, UTC). A file
without `check_result` changes no status.

- A criterion with a live test node takes its status only from its tests.
- Else the latest live file with `check_result` decides, from the files after the last reset of
  the criterion. Without `reset_on_change`, every file counts.
- `fail` is red. `pass` is green until the date `valid_until` starts, then grey
  "Manual check expired".
- Only older files give grey "Awaits checks". No file gives grey "no tests".
- At start and every 600 seconds the server turns expired checks grey, without a journal event.

## No manual acceptance

There is no manual acceptance of a node. The owner gives a decision in the chat, and the agent
records it as a Source. Green Test nodes prove the criteria. A Task does not make a criterion
green.

## Limits

A passing test can check the wrong thing or contain an empty assertion.
A record cannot show that a test is sufficient. A person must review the tests.
The criterion set can omit a significant condition. Green tests do not prove that the set is complete.
