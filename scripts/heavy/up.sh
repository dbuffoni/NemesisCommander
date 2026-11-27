#!/usr/bin/env bash
# Starts the real servers the heavy tests need: Apache mod_dav (and Nextcloud
# with NEXTCLOUD=1) with Docker Compose, MinIO and toxiproxy as programs (not
# with ONLY_DAV=1), and prints the variables the tests read:
#
#   set -a; eval "$(scripts/heavy/up.sh)"; set +a
#   PNXR_REQUIRE_SERVERS=1 cargo test -p pnxr-s3 -p pnxr-webdav --test real_server
#
# Stop them with `docker compose -f scripts/heavy/compose.yml down -v` and
# `pkill -f 'bin/minio|toxiproxy-server'`.
set -euo pipefail
cd "$(dirname "$0")"

dav_password=pnxr-dav-pass
printf 'pnxr:%s\n' "$(openssl passwd -apr1 "$dav_password")" > apache/htpasswd

profiles=()
[ "${NEXTCLOUD:-0}" = 1 ] && profiles=(--profile nextcloud)
docker compose -f compose.yml "${profiles[@]}" up -d >&2

wait_for() { # url, description
    for _ in $(seq 180); do
        curl -fs -o /dev/null "$1" && return 0
        sleep 1
    done
    echo "timed out waiting for $2" >&2
    docker compose -f compose.yml logs >&2
    exit 1
}

# Nextcloud's job needs neither MinIO nor toxiproxy.
if [ "${ONLY_DAV:-0}" != 1 ]; then
    ./install-binaries.sh >&2
    data=$(mktemp -d)
    # A bucket is a folder in MinIO's data folder.
    mkdir -p "$data/pnxr-test"
    MINIO_ROOT_USER=pnxr-access MINIO_ROOT_PASSWORD=pnxr-secret-key \
        setsid nohup bin/minio server "$data" --address 127.0.0.1:9000 >"$data.log" 2>&1 &
    wait_for http://127.0.0.1:9000/minio/health/live MinIO
    # toxiproxy: 127.0.0.1:9001 forwards to MinIO.
    setsid nohup bin/toxiproxy-server -host 127.0.0.1 -port 8474 >"$data.toxiproxy.log" 2>&1 &
    wait_for http://127.0.0.1:8474/version toxiproxy
    curl -fs -X POST http://127.0.0.1:8474/proxies \
        -d '{"name":"minio","listen":"127.0.0.1:9001","upstream":"127.0.0.1:9000"}' >/dev/null
fi
curl -s -o /dev/null -u "pnxr:$dav_password" http://127.0.0.1:8081/dav/ -w '%{http_code}' | grep -q 200 || {
    sleep 3
}

if [ "${NEXTCLOUD:-0}" = 1 ]; then
    for _ in $(seq 240); do
        curl -fs http://127.0.0.1:8082/status.php 2>/dev/null | grep -q '"installed":true' && break
        sleep 2
    done
fi

cat <<VARS
PNXR_TEST_S3_ENDPOINT=http://127.0.0.1:9000
PNXR_TEST_S3_BUCKET=pnxr-test
PNXR_TEST_S3_ACCESS_KEY=pnxr-access
PNXR_TEST_S3_SECRET_KEY=pnxr-secret-key
PNXR_TEST_DAV_ORIGIN=http://127.0.0.1:8081
PNXR_TEST_DAV_BASE=dav
PNXR_TEST_DAV_USER=pnxr
PNXR_TEST_DAV_PASSWORD=$dav_password
PNXR_TEST_TOXIPROXY=http://127.0.0.1:8474
PNXR_TEST_S3_PROXY_ENDPOINT=http://127.0.0.1:9001
VARS
if [ "${NEXTCLOUD:-0}" = 1 ]; then
    cat <<VARS
PNXR_TEST_NEXTCLOUD_ORIGIN=http://127.0.0.1:8082
PNXR_TEST_NEXTCLOUD_BASE=remote.php/dav/files/admin
PNXR_TEST_NEXTCLOUD_USER=admin
PNXR_TEST_NEXTCLOUD_PASSWORD=pnxr-test-pass-2026
VARS
fi
