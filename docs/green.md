# What green means

## The test list

A criterion names its tests in the field `tests` of its document. Save the list with
`save_object` and `data.tests`, or with `set_ac_tests {target, tests, revision, reason}`.
The block `test_results` of the workspace schema gives the form of a test name, for example
`tests/billing.rs > refund_keeps_access`. See [objects](objects.md) for each rule of the block.

Only the tests in the list count for the criterion. A test outside the list changes nothing.
One test can be in the lists of more than one criterion. One criterion can list many tests.
An inclusive threshold can need tests below, at and above the boundary.

## Records

Send each test run with `joppa_record_results {workspace, commit, results}`.
Each result gives `test` and `status`, and it can give the fields that the schema lists.
The server records the actor and the time. A caller cannot set them.
A record changes only the tests that it names. A save of a document cannot set a result.

## The rule

The schema decides the tone of a criterion from the newest result of each listed test:

- Red: one listed test has an outcome in `red_if_any`.
- Green: every listed test has an outcome in `green_if_all`.
- Grey: the list is empty, a listed test has no result, or a result is `skip` or `todo`.

With `same_commit: "fail_wins"`, a failure in any record of the newest commit decides the test.
A newer commit that passes the test clears an older failure.
Time alone never changes a tone.

## Changes

With `reset_on_change`, a change of the criterion or of a node above it makes the earlier
records stop counting. The criterion is grey until a newer record arrives.
With `draft_on_text_change`, a change of a `draft_fields` field also makes the saved list a draft.
Save the list again after you compare it with the new text.
Old records stay in the history. They are facts about the old text.

## Acceptance

`accept_commit {target, revision, commit}` records that a person accepts the requirement at one commit.
The server refuses it when a listed test has no `green_if_all` outcome on that commit.
An agent cannot accept a requirement.
A completed Task does not make a criterion green. Only `complete_task` marks a Task done.

## Limits

A passing test can check the wrong thing or contain an empty assertion.
A record cannot show that a test is sufficient. A person must review the tests.
A test that the list omits is not visible to Joppa.
The criterion set can omit a significant condition. Acceptance does not prove that the set is complete.
