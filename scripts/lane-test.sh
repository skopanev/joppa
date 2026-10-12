#!/bin/sh
# Lane gate: run all Rust tests, also the database tests, on a fresh isolated database.
# Usage: scripts/lane-test.sh ENV_FILE
# ENV_FILE is a private file outside Git. It sets JOPPA_TEST_ADMIN_URL, a postgres URL to the
# "postgres" database of the lane test server. No credential is printed.
set -eu
env_file=${1:?usage: scripts/lane-test.sh ENV_FILE}
[ -r "$env_file" ] || { echo "lane-test: the env file is not readable" >&2; exit 2; }
set -a
. "$env_file"
set +a
: "${JOPPA_TEST_ADMIN_URL:?JOPPA_TEST_ADMIN_URL is missing in the env file}"
lane=$(printf '%s' "${MEDULLA_RUN_ID:?MEDULLA_RUN_ID is required so lanes never share a database}" | tr -c 'a-zA-Z0-9_' '_' | cut -c1-40)
name="joppa_test_${lane}"
PGOPTIONS="-c client_min_messages=warning" psql "$JOPPA_TEST_ADMIN_URL" -v ON_ERROR_STOP=1 -q \
  -c "DROP DATABASE IF EXISTS \"$name\" WITH (FORCE)" \
  -c "CREATE DATABASE \"$name\"" >/dev/null
base=${JOPPA_TEST_ADMIN_URL%%\?*}
query=${JOPPA_TEST_ADMIN_URL#"$base"}
JOPPA_TEST_DATABASE_URL="${base%/*}/${name}${query}"
export JOPPA_TEST_DATABASE_URL
drop() {
  PGOPTIONS="-c client_min_messages=warning" psql "$JOPPA_TEST_ADMIN_URL" -q \
    -c "DROP DATABASE IF EXISTS \"$name\" WITH (FORCE)" >/dev/null 2>&1 || true
}
trap drop EXIT INT TERM
cargo test --lib --tests -- --include-ignored --test-threads=1
