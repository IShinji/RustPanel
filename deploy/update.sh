#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ -f "$PROJECT_ROOT/.env" ]]; then
  set -a
  source "$PROJECT_ROOT/.env"
  set +a
fi

COMPOSE_FILE="${COMPOSE_FILE:-$PROJECT_ROOT/deploy/docker-compose.ghcr.yml}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:-${COMPOSE_PROJECT_NAME:-rustpanel}}"

if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  printf '%s' "$GITHUB_TOKEN" | docker login ghcr.io -u "${GHCR_USERNAME:-_}" --password-stdin
fi

docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT" pull backend
docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT" up -d backend
