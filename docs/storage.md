# Storage and trust

A command becomes an event only after model validation inside a transaction.
A workspace advisory lock covers reading, validation and insertion. Two edits
with the same `revision` cannot both succeed. Replaying a request does
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

A retry reads the receipt and does not query events on a receipt miss.
A batch keeps one receipt with the original operations and reference labels.
Callers retry a batch with its public request ID, not an internal event ID.

Workspace renames append a canonical ID and reserve a new alias; the immutable
storage identity and old links survive. Both names resolve through the same
access checks. A name cannot be reassigned to another workspace.

A revision fixes the REQ wording, owner and entire AC set. No write edits an old
revision. Checks and discussion name the revision number. Evidence author and time come from authentication and the
server event, not caller-supplied labels. `intake.author` attributes the source
message; the authenticated writer remains separately visible in history.

## Current projections

`state_entries` stores one current entry per row. `state_revisions` stores verification records and schema revisions.
`object_revisions` stores immutable document revisions. `board_cache` stores workspace metadata and the first board view.
Card lists read separate columns without loading document bodies.
Document and history requests read only the selected object or revision.

Writes read the current validation graph. They exclude past revisions and large Note bodies from that graph.
A command on one Note loads its body before validation. Search loads bodies only for affected documents and their ancestors.
The transaction stores changed rows and affected search documents.
The journal remains the source for administrative recovery.

The isolated database test uses `pg_stat_statements` to count queries that select journal events.
It checks current reads, normal writes, retries, one selected past revision, and a large unrelated Note.
It also requires administrative recovery to produce observed journal reads and the same projection rows.
This extension runs in test databases and is not required by the production application.

Current event inserts supply their reference prefix through a transaction-local database setting.
The ID trigger uses this prefix without selecting journal events.
Each operation sets the prefix, so creation follows a preceding rename inside the same batch.
Commit and rollback clear the setting. Object registration also passes its known prefix directly.

`backfill-board` replays the journal of a workspace whose board rows are stale, under its
workspace lock, and commits all projections, including search, in one transaction. Only a
change of the board projection version makes this step slow: deployment stops the
application before it.

The status projection (`test_status` and `rollup` of `joppa.objects`) never blocks the start.
A write recomputes the nodes that it reaches: the changed nodes, each parent up to the top
and the link targets that read them. A schema change, a cycle of reached nodes or a
projection of an older version rebuilds the workspace in the write. After the start, a
background task rebuilds each workspace whose status projection is stale, one workspace in
each transaction, and then every workspace once a day. Reads give the stored rows meanwhile.

## PostgreSQL roles

Run migrations as the schema owner. Run the application with another role:

```sql
-- Supply the login password through your own secret-management process.
CREATE ROLE joppa_runtime LOGIN;
GRANT CONNECT ON DATABASE joppa TO joppa_runtime;
GRANT USAGE ON SCHEMA joppa TO joppa_runtime;
GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA joppa TO joppa_runtime;
GRANT UPDATE, DELETE ON joppa.search_documents, joppa.search_state TO joppa_runtime;
GRANT UPDATE ON joppa.board_cache, joppa.objects, joppa.state_revisions TO joppa_runtime;
GRANT UPDATE, DELETE ON joppa.state_entries TO joppa_runtime;
GRANT DELETE ON joppa.object_links TO joppa_runtime;
GRANT UPDATE(display_name) ON joppa.google_identities TO joppa_runtime;
GRANT EXECUTE ON FUNCTION joppa.purge_workspace(text) TO joppa_runtime;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA joppa TO joppa_runtime;
```

Reapply grants after additive migrations. Do not give this connection string to
end users. Business validation and authorization happen in the server; direct
INSERT is an administrative path, not a substitute for the command API.

The runtime role cannot update/delete events, own tables or disable triggers.
Even normal UPDATE, DELETE and TRUNCATE by the table owner encounter WORM
triggers. A database superuser can bypass those protections. Only the function
`joppa.purge_workspace` deletes journal rows: it removes every row of a workspace
that its owner deletes. The runtime role can only execute it. Physical storage,
backups and protection against infrastructure administrators are separate duties.

## Derived data

A change of a stored event format needs a version or a read-time conversion, never
an UPDATE of history. Do not change the meaning of recorded commands.

Search documents and their checkpoint are derived caches, rebuilt from events.
Current projections permit updates. A trigger rejects changes to stored revision contracts.
Their mutability never changes business history
or evidence. See [search freshness and recovery](search.md).
