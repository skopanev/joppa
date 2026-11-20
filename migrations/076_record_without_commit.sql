-- A record of a block with commit_required false names no commit: the commit is empty.
ALTER TABLE joppa.test_reports DROP CONSTRAINT IF EXISTS test_reports_commit_sha_check;
ALTER TABLE joppa.test_reports ADD CONSTRAINT test_reports_commit_sha_check
    CHECK (commit_sha = '' OR commit_sha ~ '^[0-9a-f]{7,64}$');
