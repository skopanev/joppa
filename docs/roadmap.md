# Product work queue

Owner: Joppa PM. Captured from the owner’s guide review on 2026-09-23 and
portal feedback relayed by r2. These are repository-tracked work items, not
implemented features or NTK ticket identifiers. No Joppa project is registered
in the NTK workspaces currently accessible to this session.

## 1. Show whether evidence applies to the current release

Priority: high. Status: planned. Start here.

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

Priority: high. Status: planned. Depends on item 1 for current-release labels.

Problem: test evidence is in the REQ-level Checks section. Readers need to see
which tests support an individual AC and understand aggregate results without
confusing passed checks with owner acceptance.

Acceptance criteria:

- Under an AC, list its declared check names and outcomes, linked to the exact
  REQ revision, run and component versions; expose expected/observed evidence.
- Distinguish no declared checks, missing results, skipped results and a running
  plan. None can produce a green number. Render the AC number grey for
  unverified, red for failure and green only for a server-verified pass.
- A REQ is green for verification only when all of its ACs pass on one subject.
  A capability aggregates every current REQ, including items hidden by filters;
  empty capabilities or empty AC sets cannot appear verified.
- Preserve owner acceptance as a separate label. If child requirements were
  checked against incompatible releases, do not imply one verified product release.
- Verify an all-pass set, a single failure, missing or skipped evidence, a new
  revision, an empty branch and filtering that hides a failing child.

Touchpoints: `src/board.js`, `src/board.css`, `tests/browser.cjs`, server summaries.

## 3. Enforce evidence-to-release matching in a pilot release pipeline

Priority: high. Status: planned. Depends on item 1’s release contract.

Problem: the administrative `joppa gate` checks accepted mandatory conditions
and compares the recorded subject with a supplied launch target. Merely having
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

Touchpoints: `src/store.rs::gate`, the pilot repository’s release pipeline.

## Deliberate non-change

Do not add automatic “cosmetic edit” exceptions to evidence invalidation in this
iteration. REQ/AC wording changes still create a revision and require fresh
results. Comments remain the place for discussion that does not change wording.
