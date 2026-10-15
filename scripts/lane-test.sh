#!/usr/bin/env bash
# Run the full suite with isolated database workers after the locked build.
if [ -z "${BASH_VERSION:-}" ]; then exec bash "$0" "$@"; fi
set -euo pipefail

if [ "${1:-}" = --worker ]; then
  # Workers share one queue: each takes the next test binary when it is free.
  queue=${2:?worker queue required}
  status=0
  while :; do
    exec 8>>"$queue.lock"
    flock 8
    next=$(cat "$queue.next")
    printf '%s\n' $((next + 1)) > "$queue.next"
    flock -u 8
    executable=$(sed -n "$((next * 2 + 1))p" "$queue")
    source=$(sed -n "$((next * 2 + 2))p" "$queue")
    [ -n "$executable" ] || break
    [ -x "$executable" ] || { echo "lane-test: missing executable: $executable" >&2; exit 2; }
    printf '     Running %s (%s)\n' "${source#"$PWD"/}" "$executable"
    "$executable" --include-ignored --test-threads=1 || status=1
  done
  exit "$status"
fi

env_file=${1:?usage: scripts/lane-test.sh ENV_FILE}
filter=${JOPPA_TEST_FILTER:-}
case "$filter" in -*) echo 'lane-test: filter must be a test name' >&2; exit 2 ;; esac
# Outside Linux the run moves into the project container through bun run.
if [ "$(uname -s)" != Linux ]; then
  if [ -n "$filter" ]; then
    exec env JOPPA_LANE_TEST_ENV="$env_file" bun run test:only "$filter" ${JOPPA_TEST_TARGET:+"$JOPPA_TEST_TARGET"}
  fi
  exec bun run test "$env_file"
fi
[ -r "$env_file" ] || { echo 'lane-test: the env file is not readable' >&2; exit 2; }
set -a
. "$env_file"
set +a
: "${JOPPA_TEST_ADMIN_URL:?JOPPA_TEST_ADMIN_URL is missing in the env file}"
if [ -f /.dockerenv ]; then
  JOPPA_TEST_ADMIN_URL=$(printf '%s' "$JOPPA_TEST_ADMIN_URL" | sed \
    -e 's/@127.0.0.1:/@host.docker.internal:/' -e 's/@localhost:/@host.docker.internal:/')
fi
case ${JOPPA_TEST_ADMIN_URL%%\?*} in
  *://*/*) ;;
  *) echo 'lane-test: JOPPA_TEST_ADMIN_URL needs a database path' >&2; exit 2 ;;
esac
lane=$(printf '%s' "${MEDULLA_RUN_ID:?MEDULLA_RUN_ID is required so lanes never share a database}" | tr -c 'a-zA-Z0-9_' '_' | cut -c1-32)
base=${JOPPA_TEST_ADMIN_URL%%\?*}
query=${JOPPA_TEST_ADMIN_URL#"$base"}
scratch=$(mktemp -d)
databases=()
pids=()
stop_groups() {
  local pid deadline running
  for pid in "$@"; do kill -TERM -- "-$pid" 2>/dev/null || true; done
  deadline=$((SECONDS + 5))
  while [ "$SECONDS" -lt "$deadline" ]; do
    running=0
    for pid in "$@"; do kill -0 -- "-$pid" 2>/dev/null && running=1; done
    [ "$running" -eq 1 ] || break
    sleep 0.05
  done
  for pid in "$@"; do
    kill -KILL -- "-$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
}
cleanup() {
  local result=$? name
  set +u
  trap - EXIT INT TERM
  stop_groups "${pids[@]}"
  for name in "${databases[@]}"; do
    if ! PGOPTIONS='-c client_min_messages=warning' psql "$JOPPA_TEST_ADMIN_URL" -v ON_ERROR_STOP=1 -q \
      -c "DROP DATABASE IF EXISTS \"$name\" WITH (FORCE)" >/dev/null; then
      echo "lane-test: could not remove isolated database $name" >&2
      [ "$result" -ne 0 ] || result=1
    fi
  done
  rm -rf "$scratch"
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
database() {
  local name="joppa_test_${lane}_$$_$1"
  PGOPTIONS='-c client_min_messages=warning' psql "$JOPPA_TEST_ADMIN_URL" -v ON_ERROR_STOP=1 -q \
    -c "CREATE DATABASE \"$name\"" >/dev/null
  databases+=("$name")
  database_url="${base%/*}/${name}${query}"
}

# Focused checks retain one database and Cargo's existing filter semantics.
status=0
if [ -n "$filter" ]; then
  database focused
  export JOPPA_TEST_DATABASE_URL="$database_url"
  case ${JOPPA_TEST_TARGET:-} in
    '') targets=(--lib --tests) ;;
    lib) targets=(--lib) ;;
    bin) targets=(--bins) ;;
    *) targets=(--test "$JOPPA_TEST_TARGET") ;;
  esac
  # Compile under the shared lock, then run the tests of the fresh build without it.
  bun run test:rust --no-run "${targets[@]}" || exit $?
  setsid env JOPPA_BUILD_LOCKED=1 bun run test:rust "${targets[@]}" "$filter" -- --include-ignored --test-threads=1 \
    > "$scratch/focused.log" 2>&1 &
  pids+=("$!")
  wait "${pids[0]}" || status=$?
  cat "$scratch/focused.log"
  if [ "$status" -eq 0 ] && ! grep -Eq '^test .+ \.\.\. (ok|FAILED)' "$scratch/focused.log"; then
    echo 'lane-test: no Rust tests match the supplied filter' >&2
    status=2
  fi
  exit "$status"
fi

# A full run writes the report for the pre-push hook. Old parts never mix with it.
rm -f .joppa/junit.xml .joppa/stamp.json .joppa/*.junit.xml .joppa/*.stamp.json
export JOPPA_TEST_ARTIFACTS="$scratch/artifacts.json"
phase=lane-tests
[ "${JOPPA_TEST_GATE:-0}" != 1 ] || phase=gate
setsid env JOPPA_TEST_COMPILE_ONLY=1 bash scripts/build.sh "$phase" "$env_file" &
pids+=("$!")
wait "${pids[0]}" || exit $?
stop_groups "${pids[@]}"
pids=()
export JOPPA_TEST_EXECUTION_ONLY=1
unset JOPPA_BUILD_LOCKED
export CARGO_TARGET_DIR="$PWD/target"
# One full suite runs at a time and gets the whole machine: parallel suites from
# several lanes only fight for CPU and the database. Compiles keep their own lock.
if [ -n "${JOPPA_BUILD_LOCK_DIR:-}" ]; then tests_lock_dir=$JOPPA_BUILD_LOCK_DIR
elif [ -f /.dockerenv ]; then tests_lock_dir=/workspace/.joppa-tooling-lock
else tests_lock_dir=$(git rev-parse --path-format=absolute --git-common-dir); tests_lock_dir=${tests_lock_dir%/.git}/.joppa-tooling-lock
fi
mkdir -p "$tests_lock_dir"
echo 'lane-test: waiting for the full-suite lock' >&2
waited=$SECONDS
exec 9>"$tests_lock_dir/tests.lock"
flock 9
echo "lane-test: full-suite lock acquired; wait $((SECONDS - waited))s" >&2
workers=$(( $(nproc) / 2 ))
[ "$workers" -ge 2 ] || workers=2
worker_ids=$(seq 0 $((workers - 1)))
# The largest test sources run first, so no worker ends with a long binary alone.
bun -e 'const a = await Bun.file(process.env.JOPPA_TEST_ARTIFACTS).json();
    if (!a.length) throw new Error("no discovered test executables");
    const size = (f) => { try { return Bun.file(f).size; } catch { return 0; } };
    a.sort((x, y) => size(y.target.src_path) - size(x.target.src_path));
    for (const t of a) { console.log(t.executable); console.log(t.target.src_path); }' > "$scratch/queue"
printf '0\n' > "$scratch/queue.next"
for worker in $worker_ids; do
  database "worker$worker"
  setsid env JOPPA_TEST_DATABASE_URL="$database_url" bash "$0" --worker "$scratch/queue" \
    > "$scratch/worker-$worker.log" 2>&1 &
  pids+=("$!")
done
for worker in $worker_ids; do
  wait "${pids[$worker]}" || status=1
  stop_groups "${pids[$worker]}"
  unset 'pids[worker]'
  cat "$scratch/worker-$worker.log"
done
pids=()
bun scripts/joppa-report/lane-junit.ts "$scratch"/worker-*.log || status=1
bun test scripts/joppa-report --reporter=junit --reporter-outfile=.joppa/bun.junit.xml || status=1
printf '{"full":true}\n' > .joppa/bun.stamp.json
# Browser checks use a third database and the helper from the same frozen build.
database browser
export JOPPA_TEST_DATABASE_URL="$database_url"
setsid bun run test:browser --test --test-reporter=junit --test-reporter-destination=.joppa/browser.junit.xml \
  --test-reporter=spec --test-reporter-destination=stdout tests/grey_ac_browser.cjs &
pids+=("$!")
wait "${pids[0]}" || status=1
printf '{"full":true}\n' > .joppa/browser.stamp.json
exit "$status"
