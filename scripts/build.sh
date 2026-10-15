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
        test-only) name=test:only; shift ;;
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
if [ "${JOPPA_TEST_EXECUTION_ONLY:-0}" = 1 ]; then
    case "${1:-}" in
        browser) shift; exec node "$@" ;;
        test)
            if [ "$#" -eq 5 ] && [ "$2" = --test ] && [ "$3" = browser_server ] \
                && [ "$4" = --no-run ] && [ "$5" = --message-format=json ]; then
                # The browser asks for its helper path. Return the frozen build result.
                exec bun -e 'const a = await Bun.file(process.env.JOPPA_TEST_ARTIFACTS).json();
                    const matches = a.filter(x => x.target.name === "browser_server");
                    if (matches.length !== 1) throw new Error("browser helper discovery is incomplete");
                    console.log(JSON.stringify(matches[0]));'
            fi
            ;;
    esac
    echo 'build: compilation is forbidden during isolated test execution' >&2
    exit 2
fi
if [ "${JOPPA_TEST_COMPILE_ONLY:-0}" != 1 ]; then
    case ${1:-} in
        gate|lane-tests)
            export JOPPA_TEST_GATE=$([ "$1" = gate ] && echo 1 || echo 0)
            unset JOPPA_TEST_FILTER
            exec bash scripts/lane-test.sh "${2:-${JOPPA_LANE_TEST_ENV:-/workspace/joppa/.local/lane-test.env}}"
            ;;
        test-only)
            # An optional test file builds only that target, not every test binary.
            { [ "$#" -eq 2 ] || [ "$#" -eq 3 ]; } && [ -n "$2" ] && [[ "$2" != -* ]] \
                || { echo 'usage: bun run test:only FILTER [TEST_FILE|lib]' >&2; exit 2; }
            if [ "$#" -eq 3 ]; then
                # "lib" names the unit tests in src; any other name is a file in tests.
                { [ "$3" = lib ] || { [[ "$3" =~ ^[a-z0-9_]+$ ]] && [ -f "tests/$3.rs" ]; }; } \
                    || { echo "build: no test file tests/$3.rs" >&2; exit 2; }
                export JOPPA_TEST_TARGET="$3"
            fi
            export JOPPA_TEST_FILTER="$2"
            exec bash scripts/lane-test.sh "${JOPPA_LANE_TEST_ENV:-/workspace/joppa/.local/lane-test.env}"
            ;;
    esac
fi
# Format errors need no compile, so they fail before the wait for the lock.
if [ "${1:-}" = lint ] && [ "${JOPPA_BUILD_LOCKED:-0}" != 1 ]; then
    cargo fmt --check
fi
# Browser checks build their helper through test:rust, which takes the lock for that compile.
# The browser run itself holds no lock.
if [ "${1:-}" = browser ]; then
    shift
    exec node "$@"
fi
if [ "${JOPPA_BUILD_LOCKED:-0}" != 1 ]; then
    [ -d "$lock_dir" ] || { echo 'build: shared lock directory is not mounted' >&2; exit 2; }
    echo 'build: waiting for the shared Rust lock' >&2
    exec flock --exclusive --close "$lock_dir/cargo.lock" env JOPPA_BUILD_LOCKED=1 \
        JOPPA_BUILD_WAIT_STARTED="$(date +%s)" bash "$root/scripts/build.sh" "$@"
fi
if [[ ${JOPPA_BUILD_WAIT_STARTED:-} =~ ^[0-9]+$ ]]; then
    echo "build: shared Rust lock acquired; wait $(($(date +%s) - JOPPA_BUILD_WAIT_STARTED))s" >&2
    unset JOPPA_BUILD_WAIT_STARTED
fi
if [ "${1:-}" != browser ] && [ -n "${1:-}" ]; then
    [ ! -L "$root/target" ] || { echo 'build: private target must not be a symlink' >&2; exit 2; }
    export CARGO_TARGET_DIR="$root/target"
    lock_hash=$(sha256sum Cargo.lock | cut -d ' ' -f 1)
    previous=$(cat "$CARGO_TARGET_DIR/.joppa-lock-sha256" 2>/dev/null || true)
    if [ "$previous" != "$lock_hash" ]; then
        if [ -d "$CARGO_TARGET_DIR" ]; then
            echo 'build: Cargo.lock fingerprint changed; clear private build artifacts' >&2
            cargo clean --target-dir "$CARGO_TARGET_DIR"
        fi
        mkdir -p "$CARGO_TARGET_DIR"
        printf '%s\n' "$lock_hash" > "$CARGO_TARGET_DIR/.joppa-lock-sha256"
    fi
fi
discover_tests() {
    : "${JOPPA_TEST_ARTIFACTS:?test artifact destination is required}"
    if ! cargo test --no-run --lib --tests --message-format=json > "$JOPPA_TEST_ARTIFACTS.raw"; then
        cat "$JOPPA_TEST_ARTIFACTS.raw" >&2
        return 1
    fi
    bun -e 'const raw = await Bun.file(process.env.JOPPA_TEST_ARTIFACTS + ".raw").text();
        const artifacts = raw.trim().split("\n").map(x => JSON.parse(x))
            .filter(x => x.reason === "compiler-artifact" && x.profile.test && x.executable);
        if (!artifacts.length) throw new Error("Cargo discovered no test executables");
        const paths = new Set();
        for (const item of artifacts) {
            if (paths.has(item.executable)) throw new Error("duplicate Cargo test executable");
            if (!item.executable.startsWith(process.env.CARGO_TARGET_DIR + "/") ||
                !item.target?.src_path || /[\r\n]/.test(item.executable + item.target.src_path))
                throw new Error("invalid Cargo test executable");
            paths.add(item.executable);
        }
        await Bun.write(process.env.JOPPA_TEST_ARTIFACTS, JSON.stringify(artifacts));'
}
case ${1:-} in
    gate)
        unset JOPPA_TEST_FILTER
        env_file=${2:-${JOPPA_LANE_TEST_ENV:-/workspace/joppa/.local/lane-test.env}}
        cargo fmt --check
        cargo clippy --all-targets -- -D warnings
        discover_tests
        ;;
    lane-tests)
        unset JOPPA_TEST_FILTER
        discover_tests
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
