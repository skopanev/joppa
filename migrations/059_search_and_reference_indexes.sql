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
-- Each statement that changes the current rows of a workspace raises its state version, also a
-- change by hand or by another process. The state that a server keeps in memory after a write
-- has this version in its key (src/store_state_cache.rs), so any other change makes the next
-- write read the rows. The function runs as the migration owner: the runtime role needs no
-- UPDATE grant on the table.
CREATE TABLE IF NOT EXISTS joppa.state_versions (
    workspace text PRIMARY KEY REFERENCES joppa.workspaces(id) ON DELETE CASCADE,
    version bigint NOT NULL
);
CREATE OR REPLACE FUNCTION joppa.raise_state_version() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
    INSERT INTO joppa.state_versions(workspace, version)
    SELECT DISTINCT c.workspace, 1 FROM changed c
    WHERE EXISTS (SELECT 1 FROM joppa.workspaces w WHERE w.id = c.workspace)
    ON CONFLICT (workspace) DO UPDATE SET version = joppa.state_versions.version + 1;
    RETURN NULL;
END $$;
CREATE OR REPLACE TRIGGER state_entries_inserted AFTER INSERT ON joppa.state_entries
    REFERENCING NEW TABLE AS changed FOR EACH STATEMENT EXECUTE FUNCTION joppa.raise_state_version();
CREATE OR REPLACE TRIGGER state_entries_updated AFTER UPDATE ON joppa.state_entries
    REFERENCING NEW TABLE AS changed FOR EACH STATEMENT EXECUTE FUNCTION joppa.raise_state_version();
CREATE OR REPLACE TRIGGER state_entries_deleted AFTER DELETE ON joppa.state_entries
    REFERENCING OLD TABLE AS changed FOR EACH STATEMENT EXECUTE FUNCTION joppa.raise_state_version();
REVOKE ALL ON joppa.state_versions FROM PUBLIC;
