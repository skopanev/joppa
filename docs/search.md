# Search for agents

`joppa_search` finds objects of every configured type. The board search field, the
HTTP route `GET /api/workspaces/{workspace}/search` and the CLI command
`joppa search` use the same search. Read a result with
`joppa_read({"item_id":"..."})` before you propose changes.

```json
{"workspace":"product","query":"restore a subscription after signing in","kind":"requirement","limit":10}
```

`workspace` and `query` are required. `kind` selects one type. The limit is 1–50,
default 10. A query is 1–2,000 characters. A copied system ID, also an old prefix
alias, matches directly and ranks first.

## Scope

Two flags set the scope. Both are false by default.

| Flag | false | true |
|---|---|---|
| `all_versions` | The current revision of each live object | Each revision of each object, archived objects too |
| `all_workspaces` | The workspace of the request | Each workspace that the reader can read, and no other |

The flags can be on together. On the board, two checkboxes, All versions and All
workspaces, send them. With a checkbox on, the board lists the results. A click on a
past revision opens that revision. A click on a result of another workspace opens
that workspace and that object.

```json
{"workspace":"product","query":"legacy export","all_versions":true,"all_workspaces":true}
```

## What is indexed

A current document holds the title of the object, its tree context and the text of
its fields. A REQ document also holds the discussion of that revision, and it marks a
withdrawn decision as withdrawn. An archived object and an object below an archived
group have no current document.

Each object revision also has a search row: its title and the fields of that
revision. A search with `all_versions` reads these rows. A revision that the current
document already covers is not a second result. A redaction rewrites the revision
row. Search results are references, never acceptance evidence.

Each document keeps at most 60,000 content characters. The embedding input is the
first 8,000 characters of the title, a line break and the content. The full source
text stays in the journal. Search is not a complete archive export.

## Full-text and vectors

Each search is hybrid. PostgreSQL keeps a GIN full-text index with simple, English
and Russian lexemes. DeepInfra computes embeddings with `Qwen/Qwen3-Embedding-8B`.
The server checks the 4,096-component response, keeps the first 1,024 components
with the Matryoshka support of the model, and normalizes them. PostgreSQL stores
`vector(1024)`. There is no local inference and no second vector database. See the
provider's [embedding API](https://deepinfra.com/Qwen/Qwen3-Embedding-8B/api) and the
model's [dimension support](https://github.com/QwenLM/Qwen3-Embedding).

Vector ranking uses exact cosine distance after the workspace and kind filter. This
avoids the filter gaps of an approximate index. Benchmark a representative corpus
before you add an ANN index. See [pgvector](https://github.com/pgvector/pgvector).

The top 100 text matches and the top 100 vector matches combine by reciprocal rank
with the constant 60. The reply gives up to `limit` results. A score is a rank value.
It is not a probability, and it is not evidence that a requirement is satisfied.

## The indexer

The server does not start without `DEEPINFRA_API_KEY`. One indexer gives each search
row a vector:

- A write commit, the server start, a retry pass every 5 minutes and a reconcile pass
  every hour wake it. It does not poll.
- The reconcile pass brings the search rows of each workspace up to its journal. It
  also renews a vector with an old profile.
- It sends at most 4 provider requests at one time and at most 200 requests in 60
  seconds, across the indexer and the queries. A request holds up to 16 texts.
- It releases the database connection before each provider request.
- It saves a vector only when the row still has the content hash of the text that it
  sent. An edit during the request cannot attach an old vector to the new text.
- A row whose hash does not change keeps its vector. A row that has the hash of
  another row with a vector copies that vector, so the provider gets no request.
- HTTP 400, 413 or 422 adds one attempt to the text. After 5 attempts the indexer
  skips that text. Another error status adds no attempt, and the other texts of the
  batch still get their vectors. A network error stops the pass until the next wake.

## The reply

| Field | Meaning |
|---|---|
| `mode` | `hybrid` when the query has a vector and the scope has vectors, otherwise `full_text` |
| `semantic_status` | `ready`, `pending` or `unavailable` |
| `indexed` / `embedded` | The rows in scope and the rows with a vector of the current profile |
| `position` | The journal position of the workspace of the request |
| `workspaces` | Each workspace that the search read |
| `items` | Each result: `workspace`, `kind`, `id`, `system_id`, `title`, `excerpt`, `req`, `revision`, `score`, `exact_id`, `historical` and `archived` |

`historical` is true when the result is not the current revision. `archived` is true
when the object has no current document. When the provider does not answer a query,
the reply keeps the full-text results and sets `semantic_status` to `unavailable`.
A successful empty search does not prove that no relevant requirement exists.

The server checks access to each workspace in scope before it sends a query to the
provider, and again after the provider answers. Indexing is a trusted server
operation. The key never reaches an agent or a browser. A revoked access also stops
the search.

## Setup and recovery

PostgreSQL 16 needs pgvector before migration 008. The provisioning script installs
the extension, and the migration enables it. For local development and CI, use the
`pgvector/pgvector:pg16` image.

Install a private `search.env` from [the template](../server/search.env.example), set
`DEEPINFRA_API_KEY`, and restart Joppa. The deploy script copies that file from the
private configuration directory. `JOPPA_EMBEDDING_URL` replaces the provider endpoint
for local tests only.

`search_documents`, `search_revisions` and `search_state` are disposable projections.
The runtime role can update and delete only these cache tables, in addition to its
SELECT and INSERT grants. Journal events, aliases, permanent IDs and command receipts
stay immutable. To rebuild a damaged projection, delete the rows of the affected
workspace from these tables in one transaction. The next search or reconcile pass
builds them again. Never edit the event journal.
