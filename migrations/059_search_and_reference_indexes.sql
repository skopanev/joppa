-- The indexer and the vector copy find the rows that wait for a vector of the current profile
-- by these partial indexes. Their predicate names embeddings::PROFILE: a new profile needs a
-- new migration with the new name in both predicates (a test of ac:2sjp3vrv checks it).
DROP INDEX IF EXISTS joppa.search_documents_pending;
CREATE INDEX IF NOT EXISTS search_documents_waiting ON joppa.search_documents(workspace) WHERE embedding IS NULL OR embedding_profile IS DISTINCT FROM 'Qwen/Qwen3-Embedding-8B:4096:MRL1024:v1';
CREATE INDEX IF NOT EXISTS search_revisions_waiting ON joppa.search_revisions(workspace) WHERE embedding IS NULL OR embedding_profile IS DISTINCT FROM 'Qwen/Qwen3-Embedding-8B:4096:MRL1024:v1';
-- The vector list of the current objects reads this index. The revisions have no vector
-- index: a search without all_versions does not read them.
CREATE INDEX IF NOT EXISTS search_documents_embedding_hnsw ON joppa.search_documents USING hnsw (embedding vector_cosine_ops) WHERE embedding IS NOT NULL;
-- The lookup of the workspaces that name a workspace reads this index for the remote links.
CREATE INDEX IF NOT EXISTS state_remote_links ON joppa.state_entries USING gin (value jsonb_path_ops) WHERE section = 'remote_links';
