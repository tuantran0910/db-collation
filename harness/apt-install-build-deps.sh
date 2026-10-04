#!/usr/bin/env bash
# Install the candidate build dependencies.
#
# `libicu-dev` must match the oracle's ICU data version. It normally comes from
# the oracle image's own apt. EOL oracle images (e.g. postgres:15-bullseye)
# ship a retired pgdg repo whose apt update fails even though the Debian
# packages are still live; passing a Debian suite (e.g. "bullseye") as $1
# repairs apt by keeping only the matching security+release sources, which carry
# the identical libicu. The runtime stage is always the untouched oracle image.
set -euo pipefail

SUITE="${1:-}"
PACKAGES=(curl ca-certificates pkg-config libicu-dev build-essential libclang-dev)

install_deps() {
    apt-get install -y --no-install-recommends "${PACKAGES[@]}"
}

if apt-get update && install_deps; then
    rm -rf /var/lib/apt/lists/*
    exit 0
fi

if [ -z "$SUITE" ]; then
    echo "apt failed and no DEBIAN_SUITE was provided to repair it" >&2
    exit 1
fi

echo "oracle apt failed; repairing sources for suite '$SUITE'" >&2
cat > /etc/apt/sources.list <<EOF
deb http://deb.debian.org/debian ${SUITE} main
deb http://deb.debian.org/debian-security ${SUITE}-security main
EOF
rm -f /etc/apt/sources.list.d/*.list
apt-get update
install_deps
rm -rf /var/lib/apt/lists/*
