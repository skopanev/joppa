-- Hybrid search: the indexer counts the provider rejections of each text, and each
-- object revision is a search row, so a search with all_versions finds old wording.
ALTER TABLE joppa.search_documents ADD COLUMN IF NOT EXISTS attempts integer NOT NULL DEFAULT 0;
ALTER TABLE joppa.search_documents ADD COLUMN IF NOT EXISTS last_error text;
CREATE TABLE IF NOT EXISTS joppa.search_revisions (
    workspace text NOT NULL REFERENCES joppa.workspaces(id),
    address text NOT NULL,
    kind text NOT NULL,
    local_id text NOT NULL,
    revision integer NOT NULL,
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
    attempts integer NOT NULL DEFAULT 0,
    last_error text,
    PRIMARY KEY (workspace, address, revision)
);
CREATE INDEX IF NOT EXISTS search_revisions_terms ON joppa.search_revisions USING gin(terms);
CREATE INDEX IF NOT EXISTS search_revisions_hash ON joppa.search_revisions(content_hash) WHERE embedding IS NOT NULL;
CREATE INDEX IF NOT EXISTS search_documents_hash ON joppa.search_documents(content_hash) WHERE embedding IS NOT NULL;
REVOKE ALL ON joppa.search_revisions FROM PUBLIC;
