#!/usr/bin/env bash
# Starts the real servers the heavy tests need (MinIO, Apache mod_dav,
# toxiproxy; Nextcloud with NEXTCLOUD=1) with Docker Compose and prints the
# variables the tests read:
#
#   set -a; eval "$(scripts/heavy/up.sh)"; set +a
#   NMC_REQUIRE_SERVERS=1 cargo test -p nmc-s3 -p nmc-webdav --test real_server
#
# Stop them with `docker compose -f scripts/heavy/compose.yml down -v`.
set -euo pipefail
cd "$(dirname "$0")"

dav_password=nmc-dav-pass
printf 'nmc:%s\n' "$(openssl passwd -apr1 "$dav_password")" > apache/htpasswd

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
wait_for http://127.0.0.1:9000/minio/health/live MinIO
# The bucket is created by the setup container.
for _ in $(seq 60); do
    [ "$(docker compose -f compose.yml ps -a --format '{{.State}}' minio-setup)" = exited ] && break
    sleep 1
done
curl -s -o /dev/null -u "nmc:$dav_password" http://127.0.0.1:8081/dav/ -w '%{http_code}' | grep -q 200 || {
    sleep 3
}

# toxiproxy: 127.0.0.1:9001 forwards to MinIO.
wait_for http://127.0.0.1:8474/version toxiproxy
curl -fs -X POST http://127.0.0.1:8474/proxies \
    -d '{"name":"minio","listen":"0.0.0.0:9001","upstream":"host.docker.internal:9000"}' >/dev/null

if [ "${NEXTCLOUD:-0}" = 1 ]; then
    for _ in $(seq 240); do
        curl -fs http://127.0.0.1:8082/status.php 2>/dev/null | grep -q '"installed":true' && break
        sleep 2
    done
fi

cat <<VARS
NMC_TEST_S3_ENDPOINT=http://127.0.0.1:9000
NMC_TEST_S3_BUCKET=nmc-test
NMC_TEST_S3_ACCESS_KEY=nmc-access
NMC_TEST_S3_SECRET_KEY=nmc-secret-key
NMC_TEST_DAV_ORIGIN=http://127.0.0.1:8081
NMC_TEST_DAV_BASE=dav
NMC_TEST_DAV_USER=nmc
NMC_TEST_DAV_PASSWORD=$dav_password
NMC_TEST_TOXIPROXY=http://127.0.0.1:8474
NMC_TEST_S3_PROXY_ENDPOINT=http://127.0.0.1:9001
VARS
if [ "${NEXTCLOUD:-0}" = 1 ]; then
    cat <<VARS
NMC_TEST_NEXTCLOUD_ORIGIN=http://127.0.0.1:8082
NMC_TEST_NEXTCLOUD_BASE=remote.php/dav/files/admin
NMC_TEST_NEXTCLOUD_USER=admin
NMC_TEST_NEXTCLOUD_PASSWORD=nmc-test-pass-2026
VARS
fi
