#!/usr/bin/env bash
# Downloads the MinIO and toxiproxy servers into scripts/heavy/bin (Linux
# x86-64, what the CI runs on). MinIO no longer publishes container images, so
# it comes from its release archive, or is built from source when that is gone.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p bin
minio_release=RELEASE.2025-04-22T22-12-26Z
if [ ! -x bin/minio ]; then
    curl -fsSL "https://dl.min.io/server/minio/release/linux-amd64/archive/minio.$minio_release" -o bin/minio \
        || GOBIN="$PWD/bin" go install github.com/minio/minio@latest
    chmod +x bin/minio
fi
if [ ! -x bin/toxiproxy-server ]; then
    curl -fsSL "https://github.com/Shopify/toxiproxy/releases/download/v2.9.0/toxiproxy-server-linux-amd64" \
        -o bin/toxiproxy-server
    chmod +x bin/toxiproxy-server
fi
