# Connect MCP

**One endpoint: `https://YOUR_JOPPA_DOMAIN/mcp`.** Our instance: `https://joppa.otion.us/mcp`.
Use HTTP (Streamable HTTP). No local Joppa client binary is needed. Add the server,
start authentication, sign in with Google and approve access. The client stores
and refreshes its OAuth credentials.

In agy: `/mcp` → joppa → Authenticate. After approval Joppa displays **Copy code**;
paste it into agy.

After approval, Joppa sends the browser to the client's callback, local
(`127.0.0.1`, `localhost`) or hosted. A client with a local listener, such as
Codex, gets the code at once. If the client runs in a container and the browser
cannot open the local address, copy the whole address from the browser into the
client's pending authorization prompt. The code is single-use and expires after
five minutes.

Claude Code, globally:

```sh
claude mcp add --transport http --scope user joppa https://YOUR_JOPPA_DOMAIN/mcp
```

Codex, in the user's `config.toml`:

```toml
[mcp_servers.joppa]
url = "https://YOUR_JOPPA_DOMAIN/mcp"
```

Then run `codex mcp login joppa` in a terminal with the same Codex configuration.
Complete browser sign-in and choose **Allow access**. The browser returns to
Codex, which stores the credentials. Then reconnect MCP. In the Codex app, use MCP server settings → **Authenticate**.

agy, globally:

```sh
agy mcp add --type http joppa https://YOUR_JOPPA_DOMAIN/mcp
```

OpenCode 1.x, in the global `opencode.jsonc`:

```json
{"mcp":{"joppa":{"type":"remote","url":"https://YOUR_JOPPA_DOMAIN/mcp","enabled":true}}}
```

## Connect an agent with a token

An agent that runs without a browser uses an agent token instead of OAuth.

1. The workspace owner opens **Settings** in the name menu, selects the workspace and issues a token for the agent.
   Select read access for an agent that only reads, for example a planner.
2. Copy the token once. Joppa does not show it again.
3. Keep the token in a file or an environment variable with access for the agent only. Never put it in a repository.
4. Send it as `Authorization: Bearer <token>` to the same endpoint.

Claude Code:

```sh
claude mcp add --transport http --scope user joppa https://YOUR_JOPPA_DOMAIN/mcp \
  --header "Authorization: Bearer $JOPPA_TOKEN"
```

Codex, in `config.toml`:

```toml
[mcp_servers.joppa]
url = "https://YOUR_JOPPA_DOMAIN/mcp"
bearer_token_env_var = "JOPPA_TOKEN"
```

A read token gets the error `read-only token` on every write. The owner revokes a token in **Settings**; the next call
with it gets HTTP 401 `token revoked`.

## Tools

| Tool | Purpose |
|---|---|
| `joppa_list` | List accessible products |
| `joppa_tasks` | List available, assigned or waiting Tasks without opening every REQ |
| `joppa_timeline` | Read every recorded change of a workspace in journal order, page by page |
| `joppa_search` | Search current wording inside one authorized workspace |
| `joppa_help` | Read command shapes and API rules |
| `joppa_tree` | Read the workspace schema and its saved revisions |
| `joppa_objects` | Read object lists, documents, links and revisions |
| `joppa_command` | Append one operation; omit IDs for new objects |
| `joppa_batch` | Append up to 100 related operations atomically |
| `joppa_tests` | Read the test status of criteria or of a Task |
| `joppa_history` | Read history pages |
| `joppa_history_mark` | Mark history entries processed |
| `joppa_record_results` | Record the test outcomes of one commit |
| `joppa_attach` | Upload a file and attach it to an object in one call |
| `joppa_attachments` | List the files of one object, also the files detached in the last 14 days |

A write cannot supply `actor`: the authenticated identity is the journal author. An `actor` field inside document data stays part of that data.

Object reads give `files`, newest first, each with its `id`, `created_at` and a
presigned `url` that is valid for 15 minutes. `joppa_attach` uploads a file of at
most 50 MiB and attaches it in one call. `joppa_attachments` lists the files of one object.

## Scripts

For Bash scripts, [client/joppa](../client/joppa) calls the same MCP endpoint.
It needs Python 3 and a private service bearer for an enrolled agent member;
the server's administrative executable is not installed on the client machine.

```sh
export JOPPA_MCP_URL=https://YOUR_JOPPA_DOMAIN/mcp
export JOPPA_TOKEN_FILE=/private/path/to/agent-token
export JOPPA_WORKSPACE=product
python3 client/joppa task list --view available
python3 client/joppa task next --request-id "$NEXT_REQUEST_ID"
python3 client/joppa task next --priority domain:billing --priority requirement:r-123
python3 client/joppa task next --dry-run --priority domain:billing
python3 client/joppa task claim TASK_ID --request-id "$CLAIM_REQUEST_ID"
python3 client/joppa task complete TASK_ID --evidence 'merge abc123'
```

Commands print JSON. `next` selects and claims under one server lock. Repeat
`--priority kind:id` in preference order. Each priority addresses an object from
the configured tree. Reference scopes follow the workspace schema. It tries the first scope with a
claimable Task, then the next, then the ordinary oldest Task. `--project`
remains a hard filter. `next --dry-run` returns `{ "task": ... }`
without claiming; `task` is `null` when nothing is available. Another worker
must atomically `claim` the returned `id`, `req` and `revision`; it may lose a
race and should retry the preview. `claim`
accepts the Task ID from the queue. Use the full Task address with `--revision N`
when an ID is ambiguous. Reuse a `--request-id` only for an identical retry. Exit code 3 means
the Task was already claimed or no matching Task is available; other errors
return 1. Do not pass the bearer token on the command line.

Each write needs a unique `request_id`. Retry identical input with the same ID
for the same result, including generated IDs. Batches can reference earlier
created objects without client-generated slugs. See [API examples](api.md#generated-ids-and-batches).

OAuth uses client registration, authorization codes and PKCE S256. Access tokens
last one hour. Refresh tokens last 30 days and rotate on use; reuse revokes the
entire family. Tokens are restricted to `/mcp`, stored server-side only as hashes
and survive deployment. Workspace access is checked for every operation.
The public origin comes from the Google callback, or explicit `JOPPA_PUBLIC_URL`.
Only loopback development origins may use HTTP.

An interactive agent acts as your account. It must approve wording, record
human decisions and accept results only on explicit instruction. Unattended
automation uses a separate `agent` identity and private service bearer token.

The server Rust executable handles serving, migrations and administration. It
is not installed to user machines and does not expose stdio MCP. Administrative
commands require `JOPPA_DATABASE_URL` and `JOPPA_ACTOR`; do not give those powers
to user agents. `gate WORKSPACE COMMIT` is a server-side release check. It passes
only when the requirement of each launch condition is accepted with `accept_commit`
on that commit.
