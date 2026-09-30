#!/usr/bin/env bash
set -euo pipefail

mode="dry-run"
retention_days="${RUNNER_CLEANUP_RETENTION_DAYS:-7}"
builder_keep_storage="${DOCKER_BUILDER_KEEP_STORAGE:-25GB}"
# 后端镜像 / 二进制的 BuildKit cache 位于 $BUILDKIT_CACHE_ROOT/<family>/<project>/,
# 由 scripts/collect-buildkit-cache-imports.ts 管理,不在这里直接删。
buildkit_cache_root="${BUILDKIT_CACHE_ROOT:-}"
buildkit_cache_project="${BUILDKIT_CACHE_PROJECT:-rustpanel}"
# 目前没有已退役的 BuildKit cache 目录 / cache mount id;留空即可,后续换 builder
# family 或改名时,把旧名字填进这两个环境变量,workflow 会自动清理一轮就能移除。
retired_buildkit_cache_dirs="${RETIRED_BUILDKIT_CACHE_DIRS:-}"
retired_buildkit_cache_ids="${RETIRED_BUILDKIT_CACHE_IDS:-}"
# backend.yml 里 setup-buildx-action 用到的命名 docker-container builder;cache mount
# 与层缓存存在各自的 buildx_buildkit_<name>0 容器里,`docker builder prune` 只作用于
# default builder,碰不到它们。
active_buildx_builders="${ACTIVE_BUILDX_BUILDERS:-rustpanel-backend-image-builder rustpanel-backend-binary-builder}"
retired_buildx_builders="${RETIRED_BUILDX_BUILDERS:-}"
# 活跃 builder 只回收这么久没用过的 cache mount(buildctl duration 格式)。
named_builder_stale_cache_mount_age="${NAMED_BUILDER_STALE_CACHE_MOUNT_AGE:-72h}"

usage() {
    cat <<'USAGE'
Usage: scripts/cleanup-self-hosted-runner.sh [--dry-run|--execute]

Safely removes stale self-hosted runner build residue. The default mode is
--dry-run, which prints intended actions without deleting data.
USAGE
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run)
            mode="dry-run"
            ;;
        --execute)
            mode="execute"
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
    shift
done

if ! [[ "$retention_days" =~ ^[1-9][0-9]*$ ]]; then
    echo "RUNNER_CLEANUP_RETENTION_DAYS must be a positive integer." >&2
    exit 2
fi

until_hours="$((retention_days * 24))h"

is_execute() {
    [ "$mode" = "execute" ]
}

run_step() {
    local label="$1"
    shift
    echo "::group::$label"
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    if is_execute; then
        "$@"
    else
        echo "Dry-run: skipped."
    fi
    echo "::endgroup::"
}

report_disk() {
    local label="$1"
    echo "::group::$label"
    df -h . || true
    if [ -n "$buildkit_cache_root" ] && [ -d "$buildkit_cache_root" ]; then
        du -sh "$buildkit_cache_root" 2>/dev/null || true
    else
        echo "BuildKit cache root not found: ${buildkit_cache_root:-<unset>}"
    fi
    echo "::endgroup::"
}

validate_cache_dir_name() {
    local name="$1"
    case "$name" in
        ""|*/*|*..*)
            echo "Invalid retired BuildKit cache dir name: $name" >&2
            return 1
            ;;
    esac
}

validate_cache_id() {
    local cache_id="$1"
    if ! [[ "$cache_id" =~ ^[A-Za-z0-9_.:-]+$ ]]; then
        echo "Invalid retired BuildKit cache id: $cache_id" >&2
        return 1
    fi
}

cleanup_retired_buildkit_local_caches() {
    if [ -z "$retired_buildkit_cache_dirs" ]; then
        echo "Retired BuildKit local cache cleanup skipped: no retired cache dirs configured."
        return
    fi
    if [ -z "$buildkit_cache_root" ]; then
        echo "Retired BuildKit local cache cleanup skipped: BUILDKIT_CACHE_ROOT not set."
        return
    fi

    for cache_dir_name in $retired_buildkit_cache_dirs; do
        validate_cache_dir_name "$cache_dir_name"
        local cache_path="$buildkit_cache_root/$cache_dir_name"
        echo "::group::Remove retired BuildKit local cache: $cache_dir_name"
        if [ ! -e "$cache_path" ]; then
            echo "Retired cache path not found: $cache_path"
        elif is_execute; then
            rm -rf "$cache_path"
        else
            echo "$cache_path"
            echo "Dry-run: skipped."
        fi
        echo "::endgroup::"
    done
}

# 列出需要扫描 staging / 隔离残留的 cache 父目录:$BUILDKIT_CACHE_ROOT/*/<project>。
# 只匹配本项目自己的目录,不触碰共享 cache root 下其他项目;本身就是 BuildKit cache
# (含 index.json)的目录跳过,避免深入 cache 内部。
stale_buildkit_cache_parents() {
    if [ -z "$buildkit_cache_root" ] || [ ! -d "$buildkit_cache_root" ]; then
        return
    fi
    validate_cache_dir_name "$buildkit_cache_project"
    local candidate
    for candidate in "$buildkit_cache_root"/*/"$buildkit_cache_project"; do
        [ -d "$candidate" ] || continue
        [ -e "$candidate/index.json" ] && continue
        printf '%s\n' "$candidate"
    done
}

cleanup_stale_buildkit_staging_caches() {
    local parents
    parents="$(stale_buildkit_cache_parents)"
    if [ -z "$parents" ]; then
        echo "BuildKit staging cache cleanup skipped: no matching directories found."
        return
    fi

    echo "::group::Remove stale BuildKit staging caches"
    local parent
    while IFS= read -r parent; do
        [ -n "$parent" ] || continue
        echo "Scanning: $parent"
        # 只清理 CI 取消或失败后遗留的 staging cache(-next-* / -prev),以及校验失败被隔离的
        # cache(*.invalid-* 由 validate-buildkit-local-cache 产生),不触碰稳定 cache。
        if is_execute; then
            find "$parent" -mindepth 1 -maxdepth 1 -type d \( -name '*-next-*' -o -name '*-prev' -o -name '*.invalid-*' \) -mtime +"$retention_days" -exec rm -rf "{}" +
        else
            find "$parent" -mindepth 1 -maxdepth 1 -type d \( -name '*-next-*' -o -name '*-prev' -o -name '*.invalid-*' \) -mtime +"$retention_days" -print
        fi
    done <<< "$parents"
    if ! is_execute; then
        echo "Dry-run: skipped."
    fi
    echo "::endgroup::"
}

cleanup_retired_buildkit_cache_mounts() {
    if [ -z "$retired_buildkit_cache_ids" ]; then
        echo "Retired BuildKit cache mount cleanup skipped: no retired cache ids configured."
        return
    fi

    for cache_id in $retired_buildkit_cache_ids; do
        validate_cache_id "$cache_id"
        run_step "Prune retired Docker builder cache mount: $cache_id" \
            docker builder prune --force --filter "id=$cache_id"
    done
}

validate_builder_name() {
    if ! [[ "$1" =~ ^[a-z0-9][a-z0-9-]*$ ]]; then
        echo "Invalid buildx builder name: $1" >&2
        exit 2
    fi
}

# 直接对 builder 容器执行 buildctl prune:buildx 的 builder 元数据按客户端(每个 runner
# 各自的 HOME)保存,新 runner 未必注册过这些 builder,而 buildx_buildkit_<name>0 容器
# 在同一个 docker daemon 上是共享的。
prune_named_buildx_builders() {
    if [ -z "$active_buildx_builders" ] && [ -z "$retired_buildx_builders" ]; then
        echo "Named buildx builder prune skipped: no builders configured."
        return
    fi
    if ! [[ "$named_builder_stale_cache_mount_age" =~ ^[1-9][0-9]*h$ ]]; then
        echo "NAMED_BUILDER_STALE_CACHE_MOUNT_AGE must look like 72h." >&2
        exit 2
    fi

    local name container
    for name in $active_buildx_builders; do
        validate_builder_name "$name"
        container="buildx_buildkit_${name}0"
        if [ "$(docker container inspect -f '{{.State.Running}}' "$container" 2>/dev/null)" != "true" ]; then
            echo "Named buildx builder prune skipped: $container is not running."
            continue
        fi
        # 只回收长时间未用的 cache mount:ci-deps 每次变化都会让 target cache mount 重新
        # seed 出一条新记录,旧记录无人再用,是 builder 膨胀的主要来源。不用
        # --keep-storage:实测它不按最近使用淘汰。
        run_step "Prune stale cache mounts in buildx builder: $name (unused > ${named_builder_stale_cache_mount_age})" \
            docker exec "$container" buildctl prune --filter "type==exec.cachemount" --keep-duration "$named_builder_stale_cache_mount_age"
    done

    for name in $retired_buildx_builders; do
        validate_builder_name "$name"
        container="buildx_buildkit_${name}0"
        if [ "$(docker container inspect -f '{{.State.Running}}' "$container" 2>/dev/null)" != "true" ]; then
            echo "Retired buildx builder prune skipped: $container is not running."
            continue
        fi
        run_step "Prune retired buildx builder cache: $name" \
            docker exec "$container" buildctl prune --all
    done
}

cleanup_runner_temp() {
    local temp_root="${RUNNER_TEMP:-}"
    if [ -z "$temp_root" ] || [ ! -d "$temp_root" ]; then
        echo "Runner temp cleanup skipped: RUNNER_TEMP is not available."
        return
    fi

    # 仅清理 GitHub Actions runner 的 _temp 目录,避免误删普通系统临时目录。
    case "$temp_root" in
        */_temp)
            ;;
        *)
            echo "Runner temp cleanup skipped: unexpected RUNNER_TEMP=$temp_root"
            return
            ;;
    esac

    echo "::group::Prune stale runner temp files"
    if is_execute; then
        find "$temp_root" -mindepth 1 -maxdepth 1 -mtime +"$retention_days" -exec rm -rf "{}" +
    else
        find "$temp_root" -mindepth 1 -maxdepth 1 -mtime +"$retention_days" -print
        echo "Dry-run: skipped."
    fi
    echo "::endgroup::"
}

echo "Self-hosted runner cleanup mode: $mode"
echo "Retention: $retention_days day(s)"
echo "Docker prune filter: until=$until_hours"
echo "Docker builder keep storage: $builder_keep_storage"
echo "BuildKit cache root: ${buildkit_cache_root:-<unset>} (project: $buildkit_cache_project)"
echo "Retired BuildKit local cache dirs: ${retired_buildkit_cache_dirs:-<none>}"
echo "Retired BuildKit cache mount ids: ${retired_buildkit_cache_ids:-<none>}"
echo "Active buildx builders: ${active_buildx_builders:-<none>} (prune cache mounts unused > ${named_builder_stale_cache_mount_age})"
echo "Retired buildx builders: ${retired_buildx_builders:-<none>}"
echo "Docker volumes are never pruned by this script."

report_disk "Disk usage before cleanup"

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    run_step "Prune stopped Docker containers" \
        docker container prune --force --filter "until=$until_hours"
    run_step "Prune dangling Docker images" \
        docker image prune --force --filter "until=$until_hours"
    run_step "Prune Docker builder cache" \
        docker builder prune --force --filter "until=$until_hours" --keep-storage "$builder_keep_storage"
    cleanup_retired_buildkit_cache_mounts
    prune_named_buildx_builders
else
    echo "Docker cleanup skipped: docker is not installed or the daemon is unavailable."
fi

cleanup_retired_buildkit_local_caches
cleanup_stale_buildkit_staging_caches
cleanup_runner_temp
report_disk "Disk usage after cleanup"
