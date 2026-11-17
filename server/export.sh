#!/usr/bin/env bash
# Usage: server/export.sh [OUTPUT_DIR]
#    or: server/export.sh SSH_TARGET PRIVATE_CONFIG_DIR [OUTPUT_DIR]
# Optional: JOPPA_SSH_CONFIG=/private/path/to/ssh_config
set -euo pipefail
umask 077

script_dir=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$script_dir/.." rev-parse --show-toplevel 2>/dev/null || (cd "$script_dir/.." && pwd))

if [[ $# -le 1 ]]; then
    : "${JOPPA_DEPLOY_HOST:?Load your private .env first}"
    target=$JOPPA_DEPLOY_HOST
    config_dir=${JOPPA_CONFIG_DIR:-}
    output_dir=${1:-${JOPPA_EXPORT_DIR:-$repo/.local/exports}}
elif [[ $# -ge 2 && $# -le 3 ]]; then
    target=$1
    config_dir=$2
    output_dir=${3:-${JOPPA_EXPORT_DIR:-$repo/.local/exports}}
else
    echo 'Usage: server/export.sh [OUTPUT_DIR]' >&2
    echo '   or: server/export.sh SSH_TARGET PRIVATE_CONFIG_DIR [OUTPUT_DIR]' >&2
    exit 2
fi

[[ "$target" != -* && "$target" =~ ^[a-zA-Z0-9_.@:-]+$ ]] || { echo 'Invalid SSH target' >&2; exit 2; }
[[ -n "$output_dir" && "$output_dir" != -* ]] || { echo 'Invalid output directory' >&2; exit 2; }

ssh_args=(-o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=yes)
[[ -z ${JOPPA_SSH_CONFIG:-} ]] || ssh_args+=(-F "$JOPPA_SSH_CONFIG")

local_stage=$(mktemp -d "${TMPDIR:-/tmp}/joppa-export.XXXXXX")
remote_stage=
cleanup() {
    local status=$?
    trap - EXIT
    if [[ -n "$remote_stage" ]]; then
        ssh "${ssh_args[@]}" "$target" "rm -rf -- '$remote_stage'" || \
            echo 'Could not remove the remote temporary export directory.' >&2
    fi
    rm -rf -- "$local_stage"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [[ -n "$config_dir" && -f "$config_dir/storage.env" ]]; then
    storage_env_text=$(cat "$config_dir/storage.env")
else
    storage_env_text=$(ssh "${ssh_args[@]}" "$target" 'cat /etc/joppa/storage.env')
fi

stamp=${JOPPA_EXPORT_STAMP:-$(date -u +%Y%m%dT%H%M%SZ)}
[[ "$stamp" =~ ^[0-9]{8}T[0-9]{6}Z$ ]] || { echo 'Invalid export timestamp' >&2; exit 2; }
remote_stage=$(ssh "${ssh_args[@]}" "$target" \
    'umask 077; install -d -m 700 -o postgres -g postgres /var/backups/joppa; runuser -u postgres -- mktemp -d /var/backups/joppa/export.XXXXXXXX')
[[ "$remote_stage" =~ ^/var/backups/joppa/export\.[a-zA-Z0-9]+$ ]] || {
    remote_stage=; echo 'Unexpected remote staging path' >&2; exit 1;
}

ssh "${ssh_args[@]}" "$target" "set -euo pipefail
cd '$remote_stage'
runuser -u postgres -- pg_dump --format=custom --no-owner --no-acl --file='$remote_stage/joppa.dump' joppa
pg_restore --list '$remote_stage/joppa.dump' >/dev/null
runuser -u postgres -- psql --no-psqlrc -v ON_ERROR_STOP=1 -Atqd joppa <<'SQL'
BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;
\set FETCH_COUNT 2000
\o raw_workspaces.jsonl
SELECT json_build_object(
  'id', w.id,
  'created_at', w.created_at,
  'aliases', COALESCE((SELECT json_agg(a.name ORDER BY a.name) FROM joppa.workspace_aliases a WHERE a.workspace = w.id), '[]'::json)
)::text
FROM joppa.workspaces w ORDER BY w.id;
\o raw_state_entries.jsonl
SELECT json_build_object(
  'workspace', workspace,
  'section', section,
  'key', key,
  'kind', kind,
  'value', value,
  'compact', compact
)::text
FROM joppa.state_entries ORDER BY workspace, section, key;
\o raw_objects.jsonl
SELECT json_build_object(
  'workspace', o.workspace,
  'address', o.address,
  'kind', o.kind,
  'local_id', o.local_id,
  'revision', o.revision,
  'parent', o.parent,
  'present', o.present,
  'title', o.title,
  'owner', o.owner,
  'description', o.description,
  'document_at', o.document_at,
  'inbox_status', o.inbox_status,
  'test_status', o.test_status,
  'document', o.document,
  'signal', o.signal,
  'process', o.process,
  'rollup', o.rollup,
  'dims', o.dims,
  'dim_tones', o.dim_tones,
  'computed', o.computed
)::text
FROM joppa.objects o ORDER BY o.workspace, o.address;
\o raw_revisions.jsonl
SELECT json_build_object(
  'workspace', workspace,
  'address', address,
  'revision', revision,
  'document', document
)::text
FROM joppa.object_revisions ORDER BY workspace, address, revision;
\o raw_test_reports.jsonl
SELECT json_build_object(
  'id', id,
  'workspace', workspace,
  'commit_sha', commit_sha,
  'commit_time', commit_time,
  'commit_order', commit_order,
  'branch', branch,
  'default_branch', default_branch,
  'dirty', dirty,
  'full_run', full_run,
  'uploader', uploader,
  'stamp_hash', stamp_hash,
  'recorded', recorded,
  'received_at', received_at,
  'junit', junit
)::text
FROM joppa.test_reports ORDER BY id;
\o raw_test_results.jsonl
SELECT json_build_object(
  'workspace', r.workspace,
  'report_id', tr.report_id,
  'address', tr.address,
  'status', tr.status,
  'duration_ms', tr.duration_ms,
  'message', tr.message,
  'status_name', tr.status_name,
  'fields', tr.fields
)::text
FROM joppa.test_results tr
JOIN joppa.test_reports r ON r.id = tr.report_id
ORDER BY tr.report_id, tr.address;
\o raw_history_marks.jsonl
SELECT json_build_object(
  'workspace', workspace,
  'position', position,
  'actor', actor,
  'alias', alias,
  'marked_at', marked_at,
  'reason', reason,
  'nodes', nodes
)::text
FROM joppa.history_marks ORDER BY workspace, position;
\o raw_events.jsonl
SELECT json_build_object(
  'id', id,
  'workspace', workspace,
  'position', position,
  'request_id', request_id,
  'actor', actor,
  'alias', alias,
  'recorded_at', recorded_at,
  'command', command
)::text
FROM joppa.events ORDER BY workspace, position;
\o
COMMIT;
SQL"

mkdir -p "$local_stage/raw"
ssh "${ssh_args[@]}" "$target" "tar -czf - -C '$remote_stage' ." | tar -xzf - -C "$local_stage/raw"
ssh "${ssh_args[@]}" "$target" "rm -rf -- '$remote_stage'"
remote_stage=

if command -v pg_restore >/dev/null 2>&1; then
    pg_restore --list "$local_stage/raw/joppa.dump" >/dev/null
elif [[ -x /opt/homebrew/opt/libpq/bin/pg_restore ]]; then
    /opt/homebrew/opt/libpq/bin/pg_restore --list "$local_stage/raw/joppa.dump" >/dev/null
fi

JOPPA_STORAGE_ENV_TEXT="$storage_env_text" \
    python3 "$script_dir/export_bundle.py" "$local_stage/raw" "$output_dir" "$stamp"
