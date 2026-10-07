# How work moves

## Who does what

People and agents formulate REQ and ACs together. Every requirement has one
owner responsible for its meaning and acceptance.

| Participant | Responsibility |
|---|---|
| REQ owner | Clarify wording, handle comments, obtain decisions and accept the result |
| Business, executive, compliance or legal specialist | Define rules and scope within their authority; record decisions in the REQ card |
| Tester or support | Report problems; identify what was checked and the observed result |
| Developer | Implement work and resolve technical questions |
| Agent | Help formulate REQ/ACs, assess readiness, organize work and run checks; ask for missing decisions |

Technical analysis is work, usually performed by an agent. An Input author does
not need to understand code, identify a build or estimate implementation. A
relevant deadline can be recorded without becoming an implementation estimate.

The agent assesses every AC when analyzing or revising a requirement. Missing
policy becomes a concrete question addressed to a person. Link an existing
applicable decision rather than repeatedly asking for it. A material change
requires a new decision. Platforms come from the agreed AC and context; see
[AC scope](model.md#ac-scope).

Each Task has one parent permitted by the workspace schema and a technical contract.
The default recipe places Tasks under ACs. Custom schemas can permit other parents and additional levels.
Triage can create unassigned Tasks before their configured parent gate permits a claim.
The MCP queue lists work that the authenticated actor can claim.
`claim_task` and `claim_next_task` assign eligible work atomically.
`release_task` returns unfinished work with a reason.
An unmet gate or dependency prevents a claim. It does not create a Task work status named blocked.
Task completion records work evidence. It never supplies a passing criterion check.

## Discussion and corrections

Found a gap? Comment in the REQ card. The owner answers there and, when needed,
updates REQ and ACs. History records the author, time and content of each saved
operation, with the reason in the discussion. No separate suggestions process.

A comment does not automatically stop work. If an answer is necessary to
implement or check an AC, mark that AC as waiting with the specific question and
recipient. Its dependents wait too; independent ready ACs may proceed.

An OAuth-connected assistant acts as the signed-in user. That credential does
not itself authorize a business decision. Confirm wording, accept results,
record specialist decisions or submit manual evidence only on explicit user
instruction. Unattended automation uses a separate agent or runner identity.

## Incoming Notes

Save new incoming information as a Note. Keep the original text, language and whitespace.
Notes use the same revisions, comments, attachments and links as other objects.
Ask for facts needed to formulate or verify the affected requirement.
Missing facts can prevent work. They do not prevent someone from recording a Note.

A message can produce several Notes. Link them to the original Note.
Record the result and its reason in the Note. Link the affected objects.
When a REQ depends on a Note's wording, link its specific revision with `depends_on`.
A later change to that Note then asks the agent to review the requirement.

## Three routes

1. **Policy request:** clarify amounts, lists and scope; formulate a REQ; have
   the authority confirm wording; analyze and implement it.
2. **Idea requiring a decision:** ask the responsible person first. Do not create
   implementation work for a disputed AC before the decision. Independent ready
   ACs can proceed. Conditions of the decision, including review dates when
   relevant, belong in the REQ and ACs.
3. **Support problem:** link an existing REQ. A confirmed defect invalidates the
   affected result; resolution and a fresh successful run restore eligibility
   for acceptance. A bug does not automatically create another requirement.

### When there is no suitable requirement

Always record the problem, then establish expected behavior.

```text
Existing REQ found              -> link it.
No REQ; behavior is undisputed  -> draft one, mark it unconfirmed, analyze work.
Expected behavior is disputed  -> request a decision; affected work waits.
```

Retrospective wording is easy to tailor to a convenient fix. It needs the same
owner confirmation as any other REQ. An incomplete tree must not prevent
Input capture. Input capture does not prove that the tree is complete.

## Readiness and results

“Ready for work” means the condition is sufficiently specified and no known
blocking question remains. “Waiting for a decision” identifies a specific
question and person. An unready AC remains part of the requirement.

Each REQ revision has its own state. Work availability, check results and owner
acceptance are separate facts.

| Summary | Meaning |
|---|---|
| Awaiting analysis | No ACs have been specified |
| N of M ACs ready for work | Those ACs can proceed; others await answers or dependencies |
| N of M ACs verified | Checks support those conditions |
| Ready for acceptance | All ACs passed on one subject; the owner must decide |
| Accepted | The owner accepted that revision and its checked result |

Silence is not approval. Confirm wording explicitly. Changed meaning requires
confirmation again. A specialist's decision may be reused only when the REQ
context and the relevant AC are unchanged; cite it in the new assessment.

## Acceptance

After checks, the REQ owner accepts the result or comments in the card. The
record identifies the author, time, revision and run. Acceptance never replaces
a missing or failing check.

Saving any REQ or AC wording change creates a new revision. All its ACs start
unverified, and it has no acceptance. Even a small edit requires fresh results.
Earlier revisions retain their checks and acceptance unchanged.

The agent compares code and tests with the new revision, changes them where
needed and reruns every AC's checks. Reuse suitable implementation and tests,
not their old results. The owner then accepts the new revision.

Corrections and withdrawn decisions append linked records. They never rewrite
history; see [WORM](model.md#immutable-history--worm).

People and agents can both miss a condition. Acceptance records what the owner
considered sufficient then; it does not prove completeness. Discuss later gaps
in the same card. Changed wording requires a new revision, fresh checks and
fresh acceptance. Historical facts remain intact.

## Repositories and tasks

The tree describes the product. A project identifies a repository where work
happens. One REQ may involve app, backend and infrastructure. Link repositories
to tasks, not to separate copies of the business tree.

A Task records its permitted parent, assignee, purpose and technical contract.
Its reference fields come from the workspace schema. A purpose can use any nonempty label.
The decision purpose retains the configured decision rules.
Completing a Task records the delivered result. It does not turn an AC green.
Decision Tasks reference the actual decision comment when the schema requires it.
Manual work still needs explicit expected and observed evidence.

## Editing guidance

Approximate character counts can help editing: 60 for domain/capability titles,
140 for a REQ title, 110 for an AC and 800 for a REQ body. Longer wording is not
rejected solely for length. Keep detailed sources linked rather than treating
the body as an entire document. Empty REQ/AC wording is rejected, and source Input text
is stored without truncation.
