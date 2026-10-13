#!/usr/bin/env bash
# Serialize Rust commands and lane checks across Joppa lanes.
set -euo pipefail
export PATH="/usr/local/cargo/bin:/home/medulla/.bun/bin:$PATH"
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
common_git=$(git rev-parse --path-format=absolute --git-common-dir)
canonical_root=${common_git%/.git}
if [ -n "${JOPPA_BUILD_LOCK_DIR:-}" ]; then
    lock_dir=$JOPPA_BUILD_LOCK_DIR
elif [ -f /.dockerenv ]; then
    lock_dir=/workspace/.joppa-tooling-lock
else
    lock_dir=$canonical_root/.joppa-tooling-lock
    mkdir -p "$lock_dir"
fi
if [ "$(uname -s)" != Linux ]; then
    mkdir -p "$lock_dir"
    mounts=(--mount "type=bind,src=$root,dst=$root"
        --mount "type=bind,src=$common_git,dst=$common_git,readonly"
        --mount "type=bind,src=$lock_dir,dst=/workspace/.joppa-tooling-lock")
    if [ "$canonical_root" != "$root" ]; then
        mounts=(--mount "type=bind,src=$canonical_root,dst=$canonical_root,readonly" "${mounts[@]}")
    fi
    case ${1:-} in
        lane-tests) name=test; shift ;;
        test) name=test:rust; shift ;;
        browser) name=test:browser; shift ;;
        build|lint|report|gate) name=$1; shift ;;
        *) echo 'build: use a configured bun run command' >&2; exit 2 ;;
    esac
    database=${JOPPA_TEST_DATABASE_URL:-}
    database=${database/@127.0.0.1:/@host.docker.internal:}
    database=${database/@localhost:/@host.docker.internal:}
    exec docker run --rm --init --add-host=host.docker.internal:host-gateway \
        "${mounts[@]}" --workdir "$root" \
        --env JOPPA_BUILD_LOCK_DIR=/workspace/.joppa-tooling-lock \
        --env "JOPPA_LANE_TEST_ENV=${JOPPA_LANE_TEST_ENV:-$canonical_root/.local/lane-test.env}" \
        --env "JOPPA_TEST_DATABASE_URL=$database" \
        --env GIT_OPTIONAL_LOCKS=0 \
        --env "MEDULLA_RUN_ID=${MEDULLA_RUN_ID:-host_$(date +%s)_$$}" \
        --entrypoint bun "${JOPPA_BUILD_IMAGE:-medulla-crew-joppa:latest}" run "$name" "$@"
fi
if [ "${JOPPA_BUILD_LOCKED:-0}" != 1 ]; then
    [ -d "$lock_dir" ] || { echo 'build: shared lock directory is not mounted' >&2; exit 2; }
    echo 'build: waiting for the shared Rust lock' >&2
    exec flock --exclusive --close "$lock_dir/cargo.lock" env JOPPA_BUILD_LOCKED=1 bash "$root/scripts/build.sh" "$@"
fi
case ${1:-} in
    gate)
        env_file=${2:-${JOPPA_LANE_TEST_ENV:-/workspace/joppa/.local/lane-test.env}}
        cargo fmt --check
        cargo clippy --all-targets -- -D warnings
        cargo build --all-targets
        exec sh scripts/lane-test.sh "$env_file"
        ;;
    lane-tests)
        exec sh scripts/lane-test.sh "${2:-${JOPPA_LANE_TEST_ENV:-/workspace/joppa/.local/lane-test.env}}"
        ;;
    lint)
        cargo fmt --check
        exec cargo clippy --all-targets -- -D warnings
        ;;
    browser)
        shift
        exec node "$@"
        ;;
    report)
        exec bun scripts/joppa-report/joppa-run.ts
        ;;
    '')
        echo 'usage: scripts/build.sh gate ENV_FILE | lane-tests ENV_FILE | report | CARGO_ARGS...' >&2
        exit 2
        ;;
    *) exec cargo "$@" ;;
esac
