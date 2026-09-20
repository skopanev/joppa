> Historical review of prototype 0.1. [Implementation status](implementation.md)
> identifies the issues addressed in 0.2 and their tests. SQL anomalies below
> concern the retained legacy schema, not the current event journal.

# Model review — 18 September 2026

Three independent reviewers examined the schema and operations. Business content
is excluded: the reviewed requirements and ACs were unconfirmed demonstration
data derived from source documents and ticket titles.

Their common finding: the tree can stay, but the prototype schema did not enforce
its promises. A short AC was being treated as sufficient admission to work.

## No complete write workflow

All three found that no tool wrote `criterion_id`, `taken_by` or `done_at`; only
manual SQL did. ACs were declared the unit of work, but tools stopped at placing
items in the tree. Only three of 124 intake points had an AC link. The two manually
set green checks were the only available completion path, not an isolated misuse.

## Tree integrity was weaker than claimed

The following live-database insertions were accepted:

| Probe | Prototype behavior |
|---|---|
| Requirement under requirement | Accepted |
| Capability under requirement | Accepted |
| Node parenting itself | Accepted; subsequently fixed |
| AC attached to a domain | Accepted but invisible in views |
| Empty or whitespace-only AC | Accepted |

`CHECK shape` validated the presence of a parent, not its kind. Cycles disappeared
from the tree instead of producing an error. AC counts only traversed requirements,
so an AC on a domain was invisible.

The proposed relational fix was a composite key `(id, kind)` and matching parent
foreign key, restricted to permitted kind pairs.

**Correction to the original review:** a foreign key targeting a partial unique
index `WHERE kind='requirement'` does not work. This was reproduced on PostgreSQL
16.15: a foreign key target must be a primary key, unique constraint or non-partial
unique index. The earlier alternative repeated another reviewer's assertion
without checking it. The current model instead validates typed commands and
workspace-local parent references before recording events.

## Two links could contradict each other

`items.node_id` could name requirement A while `items.criterion_id` named an AC
of requirement B. Neither a constraint nor a trigger rejected this.

`ON DELETE SET NULL` silently detached work when its AC was deleted. A point can
legitimately refer to a REQ before ACs are written, but when both links exist,
they must agree. Deleting an AC must not silently erase work provenance.

## Check/write races

**Correction to the original review:** calling `sort()` did not resume work after
a refusal. The legacy implementation rejected points whose outcome was not
`work`; the review initially failed to check that branch.

The actual issue was that the outcome check and subsequent write were separate
queries without a row lock. Concurrent calls could interleave. `place()` had the
same structure: two calls could create two requirements for one point, with the
last link winning and the other requirement orphaned. It also omitted kind and
required-answer validation when no outcome had been set.

Transitions need a lock or a version check in the writing transaction. Reconsidering
a refusal should record a new actor and reason, not happen as an accidental side
effect of classification with old outcome metadata left behind.

## False green

Changing AC wording retained `done_at`. Deleting a completed AC left its REQ green
by shrinking the denominator from two of two to one of one. Scope reduction was
indistinguishable from successful work.

Any UPDATE could set or clear `done_at`, and there was no `done_by`. Node-level
`taken_by`, `taken_at` and `done_at` were unused; the board ignored them. These
columns were a second apparent source of truth with no operational meaning.

Queue and tree also disagreed: routing a point to a REQ without ACs removed it
from intake while the REQ remained unready. This affected 43 of 64 requirements
in the reviewed dataset. “Placed in the tree” did not mean “ready for work.”

## Provenance proved less than promised

The original intake text was separately stored, but the path from it to the
final wording was not reconstructible. Requirements lacked authors/history;
`place()` silently reassigned points; decisions used a hard-coded pseudo-author;
answers overwrote earlier answers; freely supplied actor strings allowed
impersonation. Intake and split operations trimmed text despite promising
verbatim preservation.

Ordinary tables also offer no protection against an administrator rewriting
records. Application-role restrictions improve routine traceability but do not
establish physical immutability against infrastructure administrators.

## Intake behavior was not demonstrated

The dataset had 124 messages and 124 points, one to one. No stored example showed
`split` handling a mixed request. The data did not substantiate that workflow.

## Verification alternatives considered

The original reviewers agreed on runs, expectations and results, but differed
on how much structure to add. These alternatives are historical: the current
choice is immutable REQ revisions with AC sets, fresh checks and owner acceptance.

One proposal used a single table:

```sql
checks(id, criterion_id, at, by, method, subject, expected, observed, pass)
```

Completion would be derived from the newest check, provided it was not older than
the AC wording. Captured expectations would resist later AC changes, but the
approach did not provide the current revision model.

Another proposal separated AC revisions, runs with a plan sealed before results,
and result cells. Without a sealed plan, a reporter could insert only passing
checks and report a convenient 100% denominator.

Neither representation proves case coverage, method adequacy or truthful evidence.
Those remain acceptance judgments that someone must examine and record.

## Scale

Legacy `collect()` loaded all tables every two seconds, then scanned ACs for
every requirement. That was inexpensive at 64 requirements and 73 ACs but became
quadratic: 5,000 requirements with 20 ACs each implied about 100 million comparisons
per refresh. Grouping by requirement in one pass was the proposed fix.

Legacy `ls()` applied a limit of fifty before filtering; it could claim everything
was processed while the fifty-first item was still waiting.

## Schema reproducibility

`schema.sql` originally omitted `criteria` and columns introduced through ad hoc
ALTER statements. A clean deployment therefore created a different system.

The file was repaired and compared against live columns and 23 constraints.
The first comparison falsely passed because both queries failed and empty output
matched empty output. A corrected comparison exposed the actual discrepancy.

## Fixed at the time of that review

The schema file was made reproducible, self-parenting was rejected, and the two
incorrect assertions above were corrected. Other issues were still open then.
This historical status is not the current release status; see
[implemented fixes and tests](implementation.md).

## How to use a review

Twice, a claim copied from another reviewer became apparent evidence without a
reproduction. One was refuted by one database statement, the other by one code
branch. Claims about database or code behavior need a reproducible check before
they become implementation requirements.
