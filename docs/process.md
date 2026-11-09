# How work moves

## Who does what

People and agents formulate REQ and ACs together. Every requirement has one
owner responsible for its meaning.

| Participant | Responsibility |
|---|---|
| REQ owner | Clarify wording, handle comments and give decisions in the chat |
| Business, executive, compliance or legal specialist | Define rules and scope within their authority; record decisions in the REQ card |
| Tester or support | Report problems; identify what was checked and the observed result |
| Developer | Implement work and resolve technical questions |
| Writer agent | Talk with the owner, write the Source, REQ and AC nodes, and ask for missing decisions |
| Reviewer agent | Score each REQ and AC that another agent wrote: 11111 and Reuse |

Technical analysis is work, usually performed by an agent. The author of an Inbox item
does not need to understand code, identify a build or estimate implementation. A
relevant deadline can be recorded without becoming an implementation estimate.

The agent analyzes every AC when it analyzes or changes a requirement. For missing
policy, the agent asks a named person a specific question. Link an existing
applicable decision rather than repeatedly asking for it. A material change
requires a new decision. Platforms come from the agreed AC and context; see
[AC scope](model.md#ac-scope).

Each Task has one parent permitted by the workspace schema and a technical contract.
The recommended recipe places Tasks under ACs. Custom schemas can permit other parents and additional levels.
`data.assignee` names the member or the agent alias that does the Task.
A Task waits for its blocking dependencies, the same as any other node.
`data.commit` records the Git sha of the finished work. `archive_object` ends the Task.

A Task records work evidence. It never supplies a passing criterion check.

## Discussion and corrections

Found a gap? Comment in the REQ card. The owner answers there and, when needed,
updates REQ and ACs. History records the author, time and content of each saved
operation, with the reason in the discussion. No separate suggestions process.

A comment does not stop work. When work on an AC needs a decision, ask the
specific question with `note` and name the person who decides. A typed field,
such as `stage`, gives the state of the AC. Independent ACs can continue.

An OAuth-connected assistant acts as the signed-in user. That credential does
not itself authorize a business decision. Confirm wording, record owner and
specialist decisions or submit manual evidence only on explicit user
instruction. Unattended automation uses a separate agent or runner identity.

## Inbox items and Sources

Save each raw incoming signal as an Inbox item. Keep the original text, language and whitespace.
Inbox items use the same revisions, comments, attachments and links as other objects.
Ask for facts needed to formulate or verify the affected requirement.
Missing facts can prevent work. They do not prevent someone from recording an Inbox item.

After analysis, put the lasting content into a Source and into REQ and AC nodes.
Link the Inbox item to them, then archive it with `archive_object`.
A Source is an owner decision, a specification or an external standard with a fixed version.
Each AC names its Sources. A comment (`op: "note"`) is discussion, not a Source.

## Reading the work

The History of a workspace lists every recorded change. The agent reads the open entries with
`joppa_history` and `open: true`, or the newest entries with `order` and `after`.
After the work, the agent marks each entry with `joppa_history_mark`.

## Before implementation

Each REQ and AC passes three gates before code changes:

1. **11111.** The Reviewer agent scores the node against five rules: Grounded, Atomic,
   Unambiguous, Minimal and Verifiable. Each 0 gets a fix: STRIP, SPLIT, ASK_OPTIONS or REWRITE.
2. **Reuse (R).** The Reviewer agent searches the codebase and the Tech root for ready shared
   types, packages, libraries, helpers, modules and contracts. It uses the code graph index
   (codebase-memory, CBM) when it is connected. The Writer agent reuses them and writes no
   duplicates.
3. **Task on demand.** The coding agent creates the Tasks under the AC directly before
   implementation. One AC can have one Task or more. Do not create Tasks while the tree is
   filled or agreed: the tree changes too fast.

See [before implementation](objects.md#before-implementation) for the full rules.

## The feedback loop

Agents with different aliases do the work. The agent that writes a node never scores it.

1. The owner gives a decision in the chat. The Writer agent records it as a Source.
2. The Writer agent writes the REQ and the AC from the Source.
3. The independent Reviewer agent checks 11111 and Reuse (R). No test of the AC exists yet.
4. On a HOLD, the Writer agent does not invent an answer. A HOLD comes from a gap in the
   Source, a fork, or a conflict with an existing mechanism. The Writer agent asks the owner
   one closed question in the chat. Then it updates the Source and the AC.
5. The Reviewer agent checks 11111 and R again.
6. After a pass, two independent agents work on the AC at the same time:
   - One agent writes the code for the AC with its Tasks. One AC can have one Task or more.
   - Another agent writes the acceptance Test nodes of the AC as a black box, from the AC and
     its Sources. The coder cannot fit the code to tests that it did not see.
7. The work ends when all Test nodes of the AC are green at the same time.

This is a process rule for agents. The server does not check the roles of the aliases.

## Test nodes

Each test node states one scenario. Write it so that a person without code can read it:

- `title`: the scenario in business language, with concrete numbers and no code.
- `body`: the inputs and the expected outcome.
- `case`: `nominal` for the normal working path, `boundary` for the edges of the limits, or
  `off_nominal` for errors and safe refusals.
- `command`: the exact isolated command that runs this one test. A manual check is no test
  node: attach its evidence file to the AC with `check_result`.

Check each test node with the Test-5 checklist:

1. Business language and concrete numbers. No code in the title.
2. One test, one diagnosis.
3. The normal path, the boundaries and a safe failure.
4. The real system. No mocks of the logic under test.
5. A fast, isolated command.

All tests under one AC together cover 100% of its conditions. The AC is green only when all of
them are green at the same time. A record names each test by `test:<id>`, which binds that one
test node. A title binds every present test node with that title.

## Three routes

1. **Policy request:** clarify amounts, lists and scope; formulate a REQ; have
   the authority approve the wording; analyze and implement it.
2. **Idea requiring a decision:** ask the responsible person first. Do not create
   implementation work for a disputed AC before the decision. Independent
   ACs can proceed. Conditions of the decision, including review dates when
   relevant, belong in the REQ and ACs.
3. **Support problem:** link an existing REQ. A confirmed defect gets a test that fails on
   it. A fresh successful run makes the criterion green again. A bug does not automatically
   create another requirement.

### When there is no suitable requirement

Always record the problem, then establish expected behavior.

```text
Existing REQ found              -> link it.
No REQ; behavior is undisputed  -> draft one and analyze work.
Expected behavior is disputed  -> request a decision; affected work waits.
```

Retrospective wording is easy to tailor to a convenient fix. Ground it in a Source
as any other REQ. An incomplete tree must not prevent
a new Inbox item. A new Inbox item does not prove that the tree is complete.

## Results

Each REQ revision has its own state. Work availability and test results are separate facts.

| Summary | Meaning |
|---|---|
| Awaiting analysis | No ACs have been specified |
| N of M ACs green | Test records after the last change prove those conditions |
| Green | Every AC is green |

Silence is not approval. The owner decision is a Source, and green Test nodes prove the
criteria. There is no manual acceptance. A specialist's decision may be reused only when the
REQ context and the relevant AC are unchanged. Cite it in the new analysis.

## Changes

Saving any REQ or AC wording change creates a new revision. Its ACs are grey until a newer
test record arrives. Even a small edit needs fresh results. Earlier revisions keep their
records.

The agent compares code and tests with the new revision, changes them where needed and
reruns every AC's checks. Reuse suitable implementation and tests, not their old results.

Corrections and withdrawn decisions append linked records. They never rewrite
history; see [WORM](model.md#immutable-history--worm).

People and agents can both miss a condition. Green tests do not prove that the criteria are
complete. Discuss later gaps in the same card. Changed wording requires a new revision, an
updated Source and fresh checks. Historical facts remain intact.

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
Keep detailed source text in linked Sources or attached files, not in the body.
Empty REQ and AC wording is refused.
