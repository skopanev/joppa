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

Technical analysis is work, usually performed by an agent. An intake author does
not need to understand code, identify a build or estimate implementation. A
relevant deadline can be recorded without becoming an implementation estimate.

The agent assesses every AC when analyzing or revising a requirement. Missing
policy becomes a concrete question addressed to a person. Link an existing
applicable decision rather than repeatedly asking for it. A material change
requires a new decision. Platforms come from the agreed AC and context; see
[AC scope](model.md#ac-scope).

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

## Intake

Save the original message **verbatim**, without trimming or paraphrasing. It is
the reference for checking that later analysis preserved the author's meaning.

Classify each point as a bug, policy, question or idea, then ask only the fields
missing for that kind. Required questions live in workspace policy, not solely
in an agent prompt. The owner changes policy with `questions`; unfinished points
of that kind collect answers again while older answers remain in history.

Raw intake is valid. Reject readiness when information is missing, not the fact
that someone reported a problem.

### Split a message into points

A request for four things may lead to two implementations, one refusal and one
alternative solution. Each point gets its own outcome. Record the reason for a
refusal next to the point so it is not recreated without that context later.

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
owner confirmation as any other REQ. The tree being incomplete must not block
intake, and intake never proves the tree complete.

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

A task references concrete ACs of a revision and has an assignee and purpose:
implementation, manual work or a decision. Completing it records what was done;
it does not turn an AC green. Decision tasks reference the actual decision
comment. Manual work still needs its explicit expected/observed evidence.

## Editing guidance

Approximate character counts can help editing: 60 for domain/capability titles,
140 for a REQ title, 110 for an AC and 800 for a REQ body. Longer wording is not
rejected solely for length. Keep detailed sources linked rather than treating
the body as an entire document. Empty REQ/AC wording is rejected, and raw intake
is stored without truncation.
