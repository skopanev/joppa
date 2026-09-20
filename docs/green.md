# What green means

## The AC ID lives in the test

Put the AC identifier in the test name or annotation, beside the code. A tracker
entry containing a test filename breaks silently when the test moves. A tag
moves with the test; deleting a declared test leaves missing evidence.

```text
AC      threshold-inclusive: the 10,000.00 boundary is inclusive
Test    it('[threshold-inclusive] reports at the threshold', ...)
Run     build b1f4a2c -> check outcomes
Joppa   records the report and derives the verdict
```

The current check protocol uses the stable local AC ID returned by MCP in the
`[AC-id]` tag. Resolve a copied public system ID through `joppa_read` first.

## One test — one AC

A failure in a test claiming several ACs does not identify which condition
failed. Each automated check names exactly one AC. Joppa rejects a second AC tag.

One AC may have many tests. An inclusive threshold can need below, at and above
boundary tests; all three carry the same AC ID. An end-to-end test establishes
the condition it explicitly checks. Other conditions need their own evidence.

## The rule

An AC in revision R is green on subject S only when:

1. Its expected check set is known and nonempty.
2. Discovery completed across the declared scope.
3. Every expected check has a result for R on S.
4. Every check passed: none failed, was skipped or is missing.

“One check passed and none failed” is insufficient. Discovering only one of five
tests must not look like 100% coverage. Seal the expected plan **before** any
results, so the reporter cannot choose only successful rows afterward.

The runner declares the scope, checks and whether discovery completed. A
discovered but unexecuted check stays missing or skipped. An undiscovered test
cannot be inferred from a plan that never named it; completeness of discovery
must be supplied separately. Failed or partial discovery prevents green even
when all collected tests pass. This checks the discovery claim, not whether
the test suite is sufficient or honest.

## Repeated runs

For a revision and subject, the current verdict comes from the **latest-started
completed** run of the full expected set. Start order is journal order when the
plan was sealed, not a client timestamp or report arrival time. A late result
from an older run cannot replace a newer verdict. Until a new run finishes,
the previous completed run remains visible.

A later completed failure removes green; acceptance cannot select an earlier
successful run. A requirement revision is checked only when all its ACs pass
on **one subject**. Results from different revisions do not combine.

The subject is a map of relevant immutable component builds or configurations,
not necessarily one Git SHA. App, backend, infrastructure and published content
may all matter. The plan must identify the actual combination being checked.

Green means “checks passed.” The owner still needs to accept that revision and
run in the REQ card. See [acceptance](process.md#acceptance).

## New revisions

Every saved change to a REQ or any AC creates a revision with **all ACs at n/a**.

| State | Meaning |
|---|---|
| n/a | This revision has not been verified |
| Red | A check ran and failed |
| Green | Required checks ran and passed |

Red is observed failure; n/a is absent evidence. Old results remain facts about
old revisions. They are neither rewritten as failures nor copied forward.
A late report for an older revision cannot establish a newer one.

Compare code and tests with the new revision, update as necessary and rerun all
its checks. Rewriting suitable tests is unnecessary. New results and owner
acceptance are necessary.

## Three invariants

- Joppa derives verdicts from evidence. Missing evidence never means done.
- New revisions inherit no green or acceptance.
- Conditions that require human inspection use the same plan/result records,
  with a human author and concrete evidence.

## Limits

A passing test can check the wrong thing or contain an empty assertion. The tag
and report cannot establish semantic adequacy; someone must review them.

The AC set can omit a significant condition. Neither the schema nor owner
acceptance proves completeness. Discuss a newly found gap in the same card and,
when wording changes, verify and accept the new revision.

## Manual checks and launch

The plan contains the exact expected list. The result contains the observed
list, inspection note and optional evidence link. Authenticated actor and server
time identify who recorded it. Expected `read, verify` versus observed
`read, verify, delete` cannot pass. Manual lists are compared as sets. Missing
evidence is not a pass; only a human identity can submit manual results.

An AC needed before production has `before_launch: true`. The server-side
administrative command `joppa gate WORKSPACE < subject.json` requires acceptance
of mandatory conditions for the specified builds/configuration. Having no
launch conditions does not imply readiness. A deployment is blocked only when
its pipeline actually invokes and enforces this gate.

A subsequent plan in the same revision cannot omit a previously declared check
or change its AC, method or expectation. Changing that verification contract
requires a new revision with a reason and fresh acceptance.
