#!/usr/bin/env bash
# Run as root on a new Ubuntu 24.04 host. Credentials are configured separately.
set -euo pipefail
[[ $EUID == 0 ]] || { echo 'Run as root' >&2; exit 1; }
. /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 24.04 ]] || { echo 'Expected Ubuntu 24.04' >&2; exit 1; }
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq ca-certificates curl gnupg build-essential pkg-config postgresql-16 postgresql-server-dev-16 rsync ufw
# Use the publisher's Caddy repository, including its systemd service.
curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/gpg.key -o /tmp/joppa-caddy-key.asc
gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg /tmp/joppa-caddy-key.asc
curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt -o /etc/apt/sources.list.d/caddy-stable.list
chmod 644 /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list
apt-get update -qq
apt-get install -y -qq caddy
# Build a pinned pgvector release for the already-installed PostgreSQL 16.
if [[ ! -f /usr/share/postgresql/16/extension/vector.control ]]; then
    vector_build=$(mktemp -d /tmp/joppa-pgvector.XXXXXX)
    curl -fsSL https://github.com/pgvector/pgvector/archive/refs/tags/v0.8.6.tar.gz -o "$vector_build/source.tar.gz"
    tar -xzf "$vector_build/source.tar.gz" -C "$vector_build"
    make -C "$vector_build/pgvector-0.8.6" -j1 PG_CONFIG=/usr/lib/postgresql/16/bin/pg_config
    make -C "$vector_build/pgvector-0.8.6" install PG_CONFIG=/usr/lib/postgresql/16/bin/pg_config
fi
id joppa >/dev/null 2>&1 || useradd --system --home /var/lib/joppa --create-home --shell /usr/sbin/nologin joppa
install -d -m 750 -o root -g joppa /etc/joppa
install -d -m 755 /opt/joppa/bin /opt/joppa/server /opt/joppa/src
install -d -m 750 -o joppa -g joppa /var/lib/joppa
install -d -m 700 -o postgres -g postgres /var/backups/joppa
# One build job and a small swap area avoid starving PostgreSQL during release builds.
if ! swapon --show=NAME --noheadings | grep -q . && [[ ! -e /swapfile-joppa ]]; then
    fallocate -l 2G /swapfile-joppa
    chmod 600 /swapfile-joppa
    mkswap /swapfile-joppa
    swapon /swapfile-joppa
    printf '/swapfile-joppa none swap sw 0 0\n' >> /etc/fstab
fi
ufw allow OpenSSH
ufw allow 80/tcp
ufw allow 443/tcp
ufw --force enable
if [[ ! -x /root/.cargo/bin/cargo ]]; then
    curl -fsSL https://sh.rustup.rs -o /tmp/joppa-rustup.sh
    sh /tmp/joppa-rustup.sh -y --profile minimal
fi
printf 'Dependencies ready. PostgreSQL, pgvector and Caddy installed.\n'
