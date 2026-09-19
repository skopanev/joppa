-- psql entry point; `joppa migrate` applies the same migration transactionally.
\set ON_ERROR_STOP on
BEGIN;
SELECT pg_advisory_xact_lock(91827101);
\ir migrations/001_journal.sql
COMMIT;
