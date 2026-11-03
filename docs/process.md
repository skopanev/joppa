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

Technical analysis is work, usually performed by an agent. A Note author does
not need to understand code, identify a build or estimate implementation. A
relevant deadline can be recorded without becoming an implementation estimate.

The agent assesses every AC when analyzing or revising a requirement. Missing
policy becomes a concrete question addressed to a person. Link an existing
applicable decision rather than repeatedly asking for it. A material change
requires a new decision. Platforms come from the agreed AC and context; see
[AC scope](model.md#ac-scope).

Each Task has one parent permitted by the workspace schema and a technical contract.
The default recipe places Tasks under ACs. Custom schemas can permit other parents and additional levels.
`data.assignee` names the member who does the Task. Triage can create Tasks without an assignee.
A Task waits for its dependencies and for its readiness mark, the same as any other node.
`data.commit` records the Git sha of the finished work. `archive_object` ends the Task.

A Task records work evidence. It never supplies a passing criterion check.

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
   the authority approve the wording; analyze and implement it.
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
a new Note. A new Note does not prove that the tree is complete.

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

Saving any REQ or AC wording change creates a new revision. Its ACs are grey
until a newer test record arrives, and it has no acceptance. Even a small edit
needs fresh results. Earlier revisions keep their checks and acceptance.

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

A Task records its permitted parent, assignee and technical contract.
Its reference fields come from the workspace schema.
The `data.commit` of a Task records the delivered result. It does not turn an AC green.
Manual work needs explicit expected and observed evidence.

## Editing guidance

A title has at most `title_max_length` characters of its type, default 128.
Each other value has at most `field_max_length` characters, default 1024.
Keep detailed sources in linked Notes or attached files, not in the body.
Empty REQ and AC wording is refused.
