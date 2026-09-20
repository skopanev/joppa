-- Rebuildable search projections. Business history remains in immutable events.
CREATE EXTENSION IF NOT EXISTS vector;
CREATE TABLE IF NOT EXISTS joppa.search_state (
    workspace text PRIMARY KEY REFERENCES joppa.workspaces(id),
    position bigint NOT NULL,
    canonical_id text NOT NULL,
    reference_prefix text NOT NULL
);
CREATE TABLE IF NOT EXISTS joppa.search_documents (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    kind text NOT NULL CHECK(kind IN ('domain','capability','requirement','ac')),
    local_id text NOT NULL,
    req text,
    revision integer,
    title text NOT NULL,
    content text NOT NULL,
    content_hash text NOT NULL,
    terms tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('simple', title), 'A') ||
        setweight(to_tsvector('english', title), 'A') ||
        setweight(to_tsvector('russian', title), 'A') ||
        setweight(to_tsvector('simple', content), 'B') ||
        setweight(to_tsvector('english', content), 'B') ||
        setweight(to_tsvector('russian', content), 'B')
    ) STORED,
    embedding vector(1024),
    embedding_profile text,
    PRIMARY KEY(workspace,kind,local_id)
);
CREATE INDEX IF NOT EXISTS search_documents_terms ON joppa.search_documents USING gin(terms);
CREATE INDEX IF NOT EXISTS search_documents_pending ON joppa.search_documents(workspace) WHERE embedding IS NULL;
-- An exact distance search after the workspace filter avoids cross-tenant ANN
-- starvation and keeps this small-server index inexpensive. Benchmark before ANN.
REVOKE ALL ON joppa.search_state, joppa.search_documents FROM PUBLIC;
