#!/usr/bin/env bash
set -euo pipefail

runner_dir="${GITHUB_RUNNER_DIR:-/runner}"
runner_template_dir="${GITHUB_RUNNER_TEMPLATE_DIR:-/opt/actions-runner}"
runner_workdir="${GITHUB_RUNNER_WORKDIR:-_work}"
runner_name="${GITHUB_RUNNER_NAME:-$(hostname)}"
runner_labels="${GITHUB_RUNNER_LABELS:-m4-runner,rustpanel-runner}"

if [ -z "${GITHUB_REPOSITORY_URL:-}" ]; then
    echo "GITHUB_REPOSITORY_URL is required." >&2
    exit 2
fi

mkdir -p "$runner_dir"
if [ ! -f "$runner_dir/config.sh" ]; then
    cp -a "$runner_template_dir"/. "$runner_dir"/
fi

cd "$runner_dir"

if [ ! -f .runner ]; then
    if [ -z "${GITHUB_RUNNER_TOKEN:-}" ]; then
        echo "GITHUB_RUNNER_TOKEN is required for first-time runner registration." >&2
        exit 2
    fi

    config_args=(
        --url "$GITHUB_REPOSITORY_URL"
        --token "$GITHUB_RUNNER_TOKEN"
        --name "$runner_name"
        --work "$runner_workdir"
        --labels "$runner_labels"
        --unattended
        --replace
    )

    if [ -n "${GITHUB_RUNNER_GROUP:-}" ]; then
        config_args+=(--runnergroup "$GITHUB_RUNNER_GROUP")
    fi

    if [ "${GITHUB_RUNNER_EPHEMERAL:-false}" = "true" ]; then
        config_args+=(--ephemeral)
    fi

    ./config.sh "${config_args[@]}"
fi

exec ./run.sh
