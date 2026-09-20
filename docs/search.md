# Search for agents

`joppa_search` finds current domains, capabilities, requirements and ACs within
one authorized workspace. It is an MCP tool; the read-only portal has no search
field. Read a result with `joppa_read({"item_id":"..."})` before proposing changes.

```json
{"workspace":"product","query":"restore a subscription after signing in","kind":"requirement","limit":10}
```

`workspace` and `query` are required. `kind` optionally selects `domain`,
`capability`, `requirement` or `ac`. The limit is 1–50, default 10; a query is
1–2,000 characters. A copied system ID, including an old prefix alias, also
matches directly and ranks first.

## What is indexed

Only current wording is searched. A document contains its title, business tree
context and relevant REQ body. REQ documents also include discussion on that
revision, marking withdrawn decisions as withdrawn. Removed ACs and archived
domains/capabilities disappear from current search; their immutable history is
still readable by ID. Search results are references, never acceptance evidence.

To bound index size, each document keeps at most 60,000 content characters, and
embedding input is the first 8,000 characters of title plus content. Full source
text remains in the journal. Long source documents should stay linked rather
than pasted into a requirement. Search is not a complete archive export.

## Full-text and vectors

PostgreSQL maintains a GIN full-text index with simple, English and Russian
lexemes. Workspace and kind use the projection's composite primary key. Literal
ID lookup uses the permanent ID primary key. SQL parameters bind user input.

DeepInfra computes embeddings with `Qwen/Qwen3-Embedding-8B`. The application
validates the 4,096-component response, retains the first 1,024 components using
the model's Matryoshka support and normalizes them. PostgreSQL stores `vector(1024)`;
there is no local inference or second vector database. See the provider's
[embedding API](https://deepinfra.com/Qwen/Qwen3-Embedding-8B/api) and the model's
[dimension support](https://github.com/QwenLM/Qwen3-Embedding).

Semantic ranking currently uses exact cosine distance after filtering by
workspace and kind. This avoids approximate-index filtering gaps for a small
multi-workspace installation. It reads eligible vectors; it is not a claim of
constant-time search at millions of rows. Benchmark a representative corpus
before adding an ANN index. See [pgvector](https://github.com/pgvector/pgvector).

The top 100 text matches and top 100 vector matches combine by reciprocal rank
with constant 60. The response returns up to the requested limit. Scores are
ranking values, not probabilities or evidence that a requirement is satisfied.

## Freshness and failure behavior

Business writes update the rebuildable text projection in the same transaction.
Older workspaces are backfilled on their first authorized search, or by the
embedding worker. Search returns a projection position and reads its result set
from one repeatable database snapshot.

Embeddings are asynchronous. The worker sends at most 16 documents per request,
starts at most one provider request per second across indexing and queries, and
allows at most two requests in flight. It retries provider failures after 30
seconds without consuming a permanent attempt budget. Results install only if
the document hash still matches: an edit or archive during inference cannot
attach an old vector to new content. Reusable unchanged content keeps its vector.

The response includes:

| Field | Meaning |
|---|---|
| `mode` | `hybrid` when a query vector and indexed vectors are available; otherwise `full_text` |
| `semantic_status` | `ready`, `pending`, `not_configured` or `unavailable` |
| `indexed` / `embedded` | Current document count and embeddings using the current profile, within the requested kind |
| `position` | Workspace journal position represented by the projection |
| `items` | IDs, title, excerpt, REQ/revision where applicable, score and exact-ID flag |

With no provider key, full-text search works immediately. A provider timeout or
error also falls back to full-text, with `semantic_status: unavailable`.
Pending embeddings are reported explicitly. A successful empty search does not
prove that no relevant requirement exists.

Access is checked before sending a query to DeepInfra and again after the
network call. Results and scores are restricted to the requested workspace.
Background indexing is a trusted server operation; credentials are never exposed
to an agent or browser. Revoked workspace access also blocks search.

## Setup and recovery

PostgreSQL 16 must have pgvector installed before applying migration 008. The
server provisioning script installs the extension; the migration enables it.
For local development and CI, use the `pgvector/pgvector:pg16` image.

Install a private `search.env` using [the template](../server/search.env.example),
set `DEEPINFRA_API_KEY`, and restart Joppa. The deploy script copies that file from
the private configuration directory. Missing configuration leaves full-text
search available and semantic search explicitly disabled.

`search_documents` and `search_state` are disposable projections. The runtime
role can update/delete only these cache tables in addition to its existing
SELECT/INSERT grants. Journal events, aliases, permanent IDs and command receipts
remain immutable. To rebuild a damaged projection administratively, delete both
projection tables' rows for the affected workspace together in one transaction;
the next search or worker pass reconstructs them. Never edit the event journal.
