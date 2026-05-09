#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "Usage: scripts/warm-docker-base-images.sh <Dockerfile>..." >&2
    exit 2
fi

# 预热 self-hosted runner 的 Docker 基础镜像层，避免 CI 每次构建重复下载。
images="$(awk '
  /^FROM[[:space:]]+/ {
    image = ""
    for (i = 2; i <= NF; i += 1) {
      if ($i ~ /^--/) {
        continue
      }
      image = $i
      break
    }
    if (image != "" && image !~ /^[A-Za-z_][A-Za-z0-9_-]*$/) {
      print image
    }
  }
' "$@" | sort -u)"

if [ -z "$images" ]; then
    echo "No external Docker base images found."
    exit 0
fi

printf '%s\n' "$images" | while IFS= read -r image; do
    if docker image inspect "$image" >/dev/null 2>&1; then
        echo "Docker base image cache hit: $image"
    else
        echo "Docker base image cache miss: $image"
        docker pull "$image"
    fi
done
