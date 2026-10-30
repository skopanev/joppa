-- psql entry point; `joppa migrate` applies the same migrations transactionally.
\set ON_ERROR_STOP on
BEGIN;
SELECT pg_advisory_xact_lock(91827101);
\ir migrations/001_journal.sql
\ir migrations/002_item_ids.sql
\ir migrations/003_reference_prefix.sql
\ir migrations/004_cli_credentials.sql
\ir migrations/005_mcp_oauth.sql
\ir migrations/006_workspace_aliases.sql
\ir migrations/007_command_receipts.sql
\ir migrations/008_search.sql
\ir migrations/010_consumers.sql
\ir migrations/011_board_cache.sql
\ir migrations/012_objects.sql
\ir migrations/013_object_aliases.sql
\ir migrations/014_identity_names.sql
\ir migrations/015_current_state.sql
COMMIT;
