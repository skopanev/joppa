#!/usr/bin/env bash
# Run the full suite with two isolated database workers after the locked build.
if [ -z "${BASH_VERSION:-}" ]; then exec bash "$0" "$@"; fi
set -euo pipefail

if [ "${1:-}" = --worker ]; then
  queue=${2:?worker queue required}
  status=0
  while IFS= read -r executable; do
    IFS= read -r source || { echo 'lane-test: incomplete worker queue' >&2; exit 2; }
    [ -x "$executable" ] || { echo "lane-test: missing executable: $executable" >&2; exit 2; }
    printf '     Running %s (%s)\n' "${source#"$PWD"/}" "$executable"
    "$executable" --include-ignored --test-threads=1 || status=1
  done < "$queue"
  exit "$status"
fi

env_file=${1:?usage: scripts/lane-test.sh ENV_FILE}
filter=${JOPPA_TEST_FILTER:-}
case "$filter" in -*) echo 'lane-test: filter must be a test name' >&2; exit 2 ;; esac
if [ "$(uname -s)" != Linux ] || { [ -n "$filter" ] && [ "${JOPPA_BUILD_LOCKED:-0}" != 1 ]; }; then
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
    *) targets=(--test "$JOPPA_TEST_TARGET") ;;
  esac
  setsid bun run test:rust "${targets[@]}" "$filter" -- --include-ignored --test-threads=1 > "$scratch/focused.log" 2>&1 &
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
for worker in 0 1; do
  bun -e 'const a = await Bun.file(process.env.JOPPA_TEST_ARTIFACTS).json();
    if (!a.length) throw new Error("no discovered test executables");
    for (let i = Number(process.argv[1]); i < a.length; i += 2) {
        console.log(a[i].executable); console.log(a[i].target.src_path);
    }' "$worker" > "$scratch/queue-$worker"
  database "worker$worker"
  setsid env JOPPA_TEST_DATABASE_URL="$database_url" bash "$0" --worker "$scratch/queue-$worker" \
    > "$scratch/worker-$worker.log" 2>&1 &
  pids+=("$!")
done
for worker in 0 1; do
  wait "${pids[$worker]}" || status=1
  stop_groups "${pids[$worker]}"
  unset 'pids[worker]'
  cat "$scratch/worker-$worker.log"
done
pids=()
bun scripts/joppa-report/lane-junit.ts "$scratch/worker-0.log" "$scratch/worker-1.log" || status=1
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
