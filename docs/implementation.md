# What 0.2 implements

The discussed model runs on a separate PostgreSQL journal. The legacy schema is
retained for reading; the new model never writes through it.

| Prototype issue | Implementation | Evidence |
|---|---|---|
| Tree levels and references were unprotected | Typed commands, parent validation, workspace isolation and stable AC IDs | Model validation and PostgreSQL isolation tests |
| Wording edits kept old done_at | Immutable REQ revisions containing all ACs; fresh checks and acceptance | `revision_resets_every_ac_and_old_evidence_and_acceptance_survive` |
| Green could be assigned manually | Sealed complete plan, every expected result, one AC per automated test | Full-plan, skipped-result and multiple-tag tests |
| Owners and decisions were unauthenticated labels | Transport identity, membership, REQ owner and named specialist | Impersonation, access and HTTP actor-injection tests |
| Corrections destroyed history | Withdrawal and confirmed-defect events, then fresh checks | Decision withdrawal and issue-resolution tests |
| Work could not span repositories or manual actions | AC-linked tasks with projects and implementation/manual/decision purposes | Cross-repository and independent-AC tests |
| Retries and races changed results | Workspace transaction lock, expected revision and request identity | Concurrent revise/result/accept/issue tests |
| Historical data could be overwritten | WORM triggers and restricted runtime role | PostgreSQL update/delete/truncate and grant tests |
| Agents had to invent creation IDs | Server-generated local IDs, returned permanent IDs, immutable request receipts | Authoring retry, concurrency and revision tests |
| Related tree creation needed separate round trips | Atomic batches with explicit references to earlier generated IDs | Full rollback and batch replay tests |
| Agent titles could not be corrected before confirmation | Draft-title edits without changing authority or confirming | Draft/confirmed/archived permission tests |
| Finding wording required reading the entire tree | Authorized full-text/hybrid search with current IDs and explicit coverage | Search freshness, isolation, provider failure and stale-vector tests |
| Remote agents needed a checkout for methodology | English source documentation compiled into `joppa_model_docs` | HTTP MCP documentation smoke checks |

`tests/browser.cjs` checks the portal: navigation, copying references, older
revisions, mobile layout, safe snapshots and the notes panel. The portal writes
only three things — tree order, notes (description, comment, attachment) and the
file upload behind an attachment; structure is changed through MCP alone. `tests/http_smoke.py` covers
integration writes, acceptance, launch gates, the administrative binary and MCP.
Browser-cookie content writes are rejected; content is authored through MCP.
The owner can reorder siblings in the portal.

## Deliberate boundaries

An agent records readiness through `assess`; Joppa does not contain a model that
magically decides it. The system stores a question and recipient, validates
references to the appropriate person's decision and prevents disputed work from
starting. It cannot prove legal meaning, AC completeness or test honesty.

CI adapters use plan → result → finish. `scripts/run_checks.py` executes commands
from a complete manifest. The caller attests to discovery completeness; an
unknown omitted test cannot be detected without the repository's discovery
integration. Previously declared checks cannot disappear within a revision.

Acceptance identifies a run on a specific subject. The administrative `gate`
checks that subject against target builds/configuration. Connecting it to a
product's deployment pipeline, migrating real content and assigning access are
product onboarding work. Joppa does not assume access or decisions on their behalf.

Search uses a synchronous current-state full-text projection and asynchronous
DeepInfra embeddings in pgvector. Provider configuration is required for semantic
results; coverage and fallback are explicit. Attachment upload endpoints remain
outside the implemented workflow; their storage configuration is preparatory. The journal is replayed per workspace;
large datasets will need projections and paginated history.
