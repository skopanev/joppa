# Storage and trust

A command becomes an event only after model validation inside a transaction.
A workspace advisory lock covers reading, validation and insertion. Two edits
with the same `expected_revision` cannot both succeed. Replaying a request does
not duplicate its event.

The business journal uses `workspaces` and `events`. Separate tables store
permanent item IDs, workspace aliases, browser sessions and OAuth token hashes.
Revocations append records rather than changing credentials in place. Event
payloads are typed JSON; unknown command fields are rejected. References are
scoped to a workspace: a foreign REQ, AC, project or person cannot be selected
by guessing an ID. Derived state is not another writable source of truth.

`command_receipts` stores original inputs and resolved results atomically with
new events. This preserves generated IDs across retries and server restarts.
A batch uses one transaction and lock, but one event per operation. An error
rolls back all events, generated IDs, aliases and the receipt. Receipt lookup
occurs after access checks; knowing a request ID never grants access.

Workspace renames append a canonical ID and reserve a new alias; the immutable
storage identity and old links survive. Both names resolve through the same
access checks. A name cannot be reassigned to another workspace.

A revision fixes the REQ wording, owner and entire AC set. `revise` never edits
an old revision. Assessments, checks, acceptance and discussion reference the
revision number. Evidence author and time come from authentication and the
server event, not caller-supplied labels. `intake.author` attributes the source
message; the authenticated writer remains separately visible in history.

## PostgreSQL roles

Run migrations as the schema owner. Run the application with another role:

```sql
-- Supply the login password through your own secret-management process.
CREATE ROLE joppa_runtime LOGIN;
GRANT CONNECT ON DATABASE joppa TO joppa_runtime;
GRANT USAGE ON SCHEMA joppa TO joppa_runtime;
GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA joppa TO joppa_runtime;
GRANT UPDATE, DELETE ON joppa.search_documents, joppa.search_state TO joppa_runtime;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA joppa TO joppa_runtime;
```

Reapply grants after additive migrations. Do not give this connection string to
end users. Business validation and authorization happen in the server; direct
INSERT is an administrative path, not a substitute for the command API.

The runtime role cannot update/delete events, own tables or disable triggers.
Even normal UPDATE, DELETE and TRUNCATE by the table owner encounter WORM
triggers. A database superuser can bypass those protections. Physical storage,
backups and protection against infrastructure administrators are separate duties.

## Compatibility

The 0.1 schema remains in `public`; migrations neither import nor overwrite it.
Old `done_at` values cannot become verified evidence automatically, and free-text
`by` values cannot become authenticated human decisions. The administrative
`joppa legacy serve PORT` command can expose the old read-only board.

Business migration means assigning a workspace and owners, importing wording as
new revisions, confirming it and collecting evidence. Changing a persisted event
format requires a version or read-time conversion, never an UPDATE to history.
Do not silently change the meaning of already-recorded commands.

Search documents and their checkpoint are derived caches, rebuilt from events.
They alone permit update/delete. Their mutability never changes business history
or evidence. See [search freshness and recovery](search.md).
