# Product work queue

Owner: Joppa PM. Captured from the owner’s guide review on 2026-09-23 and
portal feedback relayed by r2. These are repository planning notes; active work
belongs in Joppa Tasks. They are not implemented features.

## 1. Show whether evidence applies to the current release

Priority: high. Status: partly implemented. A newer unfinished run now appears
as in progress while the prior completed result remains historical. The current
release record and version comparison below remain open. Start there.

Problem: the portal summarizes the latest-started completed run for a REQ
revision. It does not compare the checked component versions with a currently
deployed release. A green historical run can therefore be read as evidence
about a different production build.

Acceptance criteria:

- Record the current release per product/environment as an immutable component
  version map, with authenticated provenance. Define who may report deployment
  and which component/configuration/fixture keys are required before implementation.
- Show the checked version combination beside the intended/current release and
  identify matching, differing and unknown coverage. A missing release or missing
  required component cannot imply current verification.
- When a relevant component changes, retain historical pass and acceptance but
  show that the new combination is unverified until its own evidence exists.
  Do not copy results, rewrite history or create a wording revision for a build change.
- Keep pending runs and historical runs distinguishable. Reuse the existing
  server verdict rules; do not independently infer pass from task completion or
  whichever subset of test reports arrived.
- Verify matching versions, one changed component, incomplete release metadata,
  a new wording revision and an in-progress run while an older pass exists.

Touchpoints: `src/checks.rs`, `src/store.rs`, `src/model.rs`, `src/board.js`.

## 2. Put check evidence under each AC and summarize it up the tree

Priority: high. Status: partly implemented. Status circles now show AC and
REQ status and aggregate current children up to Capability and Domain, including
filtered-out children; empty branches cannot become green. Per-AC check lists
are now visible, including expected and observed evidence; remaining work is
compatibility of evidence across releases and freshness policy integration.
Depends on item 1
for current-release labels.

Problem: test evidence is in the REQ-level Checks section. Readers need to see
which tests support an individual AC and understand aggregate results without
confusing passed checks with owner acceptance.

Acceptance criteria:

- Under an AC, list its declared check names and outcomes, linked to the exact
  REQ revision, run and component versions; expose expected/observed evidence.
- Distinguish no declared checks, missing results, skipped results and a running
  plan. None can produce a green circle. Render the AC circle grey for
  unverified, yellow for pending decisions or work, red for failure, and green
  for a server-verified pass. Elapsed time alone does not expire a pass.
  Current evidence must match the context revision and requested subject.
- A REQ is green for verification only when all of its ACs pass on one subject.
  A capability aggregates every current REQ, including items hidden by filters;
  empty capabilities or empty AC sets cannot appear verified.
- Preserve owner acceptance as a separate label; it must not gate the green
  verification circle. If child requirements were
  checked against incompatible releases, do not imply one verified product release.
- Verify an all-pass set, a single failure, missing or skipped evidence, a new
  revision, an empty branch and filtering that hides a failing child.

Touchpoints: `src/board.js`, `src/board.css`, `tests/browser.cjs`, server summaries.

## 3. Enforce evidence-to-release matching in a pilot release pipeline

Priority: high. Status: planned. Depends on item 1’s release contract.

Problem: the administrative `joppa gate WORKSPACE COMMIT` checks that the
requirement of each launch condition is accepted with `accept_commit` on the
supplied commit. Merely having
that command does not stop a pipeline that never invokes it.

Acceptance criteria:

- Choose one pilot product/environment with its owner; explicitly agree which
  ACs are mandatory before launch rather than marking every imported AC mandatory.
- Build the target version map from actual immutable release artifacts and
  configuration; validate completeness against the agreed component contract.
- Enforce the gate before promotion. No mandatory conditions, unaccepted checks,
  unknown versions or a mismatched relevant component must prevent promotion
  with a useful reason, not silently pass or continue after a failed command.
- A matching, accepted target can promote. Record the deployed combination only
  after deployment succeeds; a failed deployment must not become the current release.
- Demonstrate a blocked mismatch and a successful verified release in the pilot
  environment, without inventing human acceptance or altering historical evidence.

Touchpoints: `src/launch_gate.rs`, the pilot repository’s release pipeline.

## 4. Owner and common object model

Status: implemented. Current documents use `owner` for every configured type.
The workspace schema defines names, parent rules, reference fields and colors.
Tasks follow the configured parent rules. Inputs use common document revisions.
Historical commands retain their earlier field names for journal replay.
The public Domain commands also accept `authority` as a compatibility alias.
The common `save_object` operation uses `owner`.

Verification: `tests/schema_objects.rs`, `tests/configured_references.rs`,
`tests/custom_verification.rs`, `tests/generic_tasks.rs` and their database checks.
Browser checks cover common cards, schema tabs, lazy reads and input queues.
Current product requirements belong in the Joppa workspace. These source files
supply evidence after their checks run against a recorded commit.

## Deliberate non-change

Do not add automatic “cosmetic edit” exceptions to evidence invalidation in this
iteration. REQ/AC wording changes still create a revision and require fresh
results. Comments remain the place for discussion that does not change wording.
