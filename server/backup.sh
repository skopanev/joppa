#!/usr/bin/env bash
# Runs as postgres; systemd loads /etc/joppa/backup.env as root.
set -euo pipefail
umask 077
: "${SPACES_BUCKET:?}" "${SPACES_ENDPOINT:?}" "${SPACES_REGION:?}"
: "${AWS_ACCESS_KEY_ID:?}" "${AWS_SECRET_ACCESS_KEY:?}"
[[ "$SPACES_ENDPOINT" == https://* ]] || { echo 'Spaces endpoint must use HTTPS' >&2; exit 1; }
backup_dir=/var/backups/joppa
install -d -m 700 "$backup_dir"
exec 9>"$backup_dir/.backup.lock"
flock -n 9 || exit 0
stamp=$(date -u +%Y%m%dT%H%M%SZ)
temporary=$(mktemp "$backup_dir/.joppa-$stamp.XXXXXX")
trap 'rm -f "$temporary"' EXIT
pg_dump --format=custom --no-owner --no-acl --file="$temporary" joppa
pg_restore --list "$temporary" >/dev/null
dump="$backup_dir/joppa-$stamp.dump"
mv "$temporary" "$dump"
put() {
    curl --fail --silent --show-error --connect-timeout 10 --max-time 300 \
        --retry 3 --retry-delay 5 --aws-sigv4 "aws:amz:${SPACES_REGION}:s3" \
        --user "${AWS_ACCESS_KEY_ID}:${AWS_SECRET_ACCESS_KEY}" \
        --upload-file "$dump" "${SPACES_ENDPOINT%/}/${SPACES_BUCKET}/$1"
}
put "postgres/joppa-$stamp.dump"
put "postgres/latest.dump"
printf 'Backup uploaded: joppa-%s.dump\n' "$stamp"
# Delete only our completed daily dumps, never unrelated files or failed partials.
find "$backup_dir" -maxdepth 1 -type f -name 'joppa-????????T??????Z.dump' -mtime +14 -delete
