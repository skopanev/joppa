#!/usr/bin/env bash
# Usage: server/deploy.sh SSH_TARGET DOMAIN PRIVATE_CONFIG_DIR [INITIAL_DUMP]
# Optional: JOPPA_SSH_CONFIG=/private/path/to/ssh_config
set -euo pipefail
umask 077
if [[ $# == 0 ]]; then
    : "${JOPPA_DEPLOY_HOST:?Load your private .env first}" "${JOPPA_DOMAIN:?}" "${JOPPA_CONFIG_DIR:?}"
    set -- "$JOPPA_DEPLOY_HOST" "$JOPPA_DOMAIN" "$JOPPA_CONFIG_DIR"
    [[ -z ${JOPPA_INITIAL_DUMP:-} ]] || set -- "$@" "$JOPPA_INITIAL_DUMP"
fi
[[ $# -ge 3 && $# -le 4 ]] || {
    echo 'Usage: server/deploy.sh SSH_TARGET DOMAIN PRIVATE_CONFIG_DIR [INITIAL_DUMP]' >&2
    exit 2
}
target=$1 domain=$2 config_dir=$3 initial_dump=${4:-}
[[ "$target" != -* && "$target" =~ ^[a-zA-Z0-9_.@:-]+$ ]] || { echo 'Invalid SSH target' >&2; exit 2; }
[[ "$domain" =~ ^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$ && "$domain" == *.* ]] || {
    echo 'Expected a DNS hostname without a scheme or path' >&2; exit 2;
}
repo=$(git -C "$(dirname "$0")/.." rev-parse --show-toplevel)
[[ -z $(git -C "$repo" status --porcelain --untracked-files=normal) ]] || {
    echo 'Commit changes first: deployment ships only the clean HEAD commit.' >&2; exit 1;
}
for name in tokens.json login-users.json; do
    [[ -f "$config_dir/$name" ]] || { echo "Missing private config: $name" >&2; exit 1; }
done
[[ -z "$initial_dump" || -f "$initial_dump" ]] || { echo 'Initial dump not found' >&2; exit 1; }
ssh_args=(-o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=yes)
[[ -z ${JOPPA_SSH_CONFIG:-} ]] || ssh_args+=(-F "$JOPPA_SSH_CONFIG")
local_stage=$(mktemp -d "${TMPDIR:-/tmp}/joppa-deploy.XXXXXX")
remote_stage=
cleanup() {
    local status=$?
    trap - EXIT
    if [[ -n "$remote_stage" ]]; then
        ssh "${ssh_args[@]}" "$target" "rm -rf -- '$remote_stage'" || \
            echo 'Could not remove the remote private deployment staging directory.' >&2
    fi
    rm -rf -- "$local_stage"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
git -C "$repo" archive HEAD > "$local_stage/source.tar"
mkdir "$local_stage/private"
for name in tokens.json login-users.json google-client.json access-rules.json backup.env storage.env; do
    if [[ -f "$config_dir/$name" ]]; then
        cp "$config_dir/$name" "$local_stage/private/$name"
        chmod 600 "$local_stage/private/$name"
    fi
done
[[ -z "$initial_dump" ]] || cp "$initial_dump" "$local_stage/initial.dump"
remote_stage=$(ssh "${ssh_args[@]}" "$target" 'umask 077; mktemp -d /root/joppa-deploy.XXXXXXXX')
[[ "$remote_stage" =~ ^/root/joppa-deploy\.[a-zA-Z0-9]+$ ]] || {
    remote_stage=; echo 'Unexpected remote staging path' >&2; exit 1;
}
COPYFILE_DISABLE=1 tar --no-xattrs -C "$local_stage" -czf - . | ssh "${ssh_args[@]}" "$target" "tar -xzf - -C '$remote_stage'"
ssh "${ssh_args[@]}" "$target" \
    "mkdir '$remote_stage/source'; tar -xf '$remote_stage/source.tar' -C '$remote_stage/source'; bash '$remote_stage/source/server/install.sh' '$domain' '$remote_stage'"
curl --fail --silent --show-error --retry 12 --retry-all-errors --retry-delay 5 \
    --connect-timeout 10 --max-time 15 "https://$domain/health"
printf '\nDeployed %s at https://%s\n' "$(git -C "$repo" rev-parse --short HEAD)" "$domain"
