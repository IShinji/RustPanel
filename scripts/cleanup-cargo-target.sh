#!/usr/bin/env bash
set -euo pipefail

mode="dry-run"
retention_days="${CARGO_SWEEP_RETENTION_DAYS:-14}"
project_path="${CARGO_SWEEP_PROJECT_PATH:-src/backend}"
manual_maxsize="${CARGO_SWEEP_MAXSIZE:-}"
auto_min_free_gib="${CARGO_SWEEP_AUTO_MIN_FREE_GIB:-20}"
auto_max_target_gib="${CARGO_SWEEP_AUTO_MAX_TARGET_GIB:-60}"

usage() {
    cat <<'USAGE'
Usage: scripts/cleanup-cargo-target.sh [--dry-run|--execute|--auto] [--maxsize SIZE]

Runs cargo-sweep against the Rust backend target directory. The default mode is
--dry-run, which prints intended actions without deleting build artifacts.

Modes:
  --dry-run   Preview retention cleanup without deleting artifacts.
  --execute   Run retention cleanup; optionally also run --maxsize cleanup.
  --auto      Run before backend checks. It performs retention cleanup, then
              enforces disk/target thresholds only when needed.

Environment:
  CARGO_SWEEP_RETENTION_DAYS       Days to keep recent artifacts. Default: 14.
  CARGO_SWEEP_PROJECT_PATH         Cargo project path. Default: src/backend.
  CARGO_SWEEP_MAXSIZE              Optional max target size for --execute.
  CARGO_SWEEP_AUTO_MIN_FREE_GIB    Minimum free disk for --auto. Default: 20.
  CARGO_SWEEP_AUTO_MAX_TARGET_GIB  Target cap for --auto. Default: 60.
  CARGO_TARGET_DIR                 Optional absolute shared Cargo target dir.
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
        --auto)
            mode="auto"
            ;;
        --maxsize)
            shift
            if [ "$#" -eq 0 ]; then
                echo "--maxsize requires a value." >&2
                usage >&2
                exit 2
            fi
            manual_maxsize="$1"
            ;;
        --auto-min-free-gib)
            shift
            if [ "$#" -eq 0 ]; then
                echo "--auto-min-free-gib requires a value." >&2
                usage >&2
                exit 2
            fi
            auto_min_free_gib="$1"
            ;;
        --auto-max-target-gib)
            shift
            if [ "$#" -eq 0 ]; then
                echo "--auto-max-target-gib requires a value." >&2
                usage >&2
                exit 2
            fi
            auto_max_target_gib="$1"
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

is_positive_integer() {
    [[ "$1" =~ ^[1-9][0-9]*$ ]]
}

if ! is_positive_integer "$retention_days"; then
    echo "CARGO_SWEEP_RETENTION_DAYS must be a positive integer." >&2
    exit 2
fi
if ! is_positive_integer "$auto_min_free_gib"; then
    echo "CARGO_SWEEP_AUTO_MIN_FREE_GIB must be a positive integer." >&2
    exit 2
fi
if ! is_positive_integer "$auto_max_target_gib"; then
    echo "CARGO_SWEEP_AUTO_MAX_TARGET_GIB must be a positive integer." >&2
    exit 2
fi
if [ -n "$manual_maxsize" ] && ! [[ "$manual_maxsize" =~ ^[1-9][0-9]*([KMGT]i?B|[KMGT]B)?$ ]]; then
    echo "CARGO_SWEEP_MAXSIZE / --maxsize must look like 500, 40GB, or 60GiB." >&2
    exit 2
fi

root_dir="$(git rev-parse --show-toplevel)"
case "$project_path" in
    /*)
        sweep_path="$project_path"
        ;;
    *)
        sweep_path="$root_dir/$project_path"
        ;;
esac

if [ ! -d "$sweep_path" ]; then
    echo "Cargo sweep project path does not exist: $sweep_path" >&2
    exit 2
fi

case "$sweep_path/" in
    "$root_dir"/*)
        ;;
    *)
        echo "Cargo sweep project path must stay inside the repository: $sweep_path" >&2
        exit 2
        ;;
esac

if [ ! -f "$sweep_path/Cargo.toml" ]; then
    echo "Cargo sweep project path must contain Cargo.toml: $sweep_path" >&2
    exit 2
fi

if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    case "$CARGO_TARGET_DIR" in
        /*)
            target_dir="$CARGO_TARGET_DIR"
            ;;
        *)
            echo "CARGO_TARGET_DIR must be an absolute path for cleanup-cargo-target.sh." >&2
            exit 2
            ;;
    esac
else
    # sweep_path 若是 workspace member(如 src/backend),cargo 实际把 target/ 解析到
    # workspace 根目录,不是 sweep_path/target;问 cargo metadata 拿权威路径,拿不到才退回
    # 朴素猜测。之前直接猜 "$sweep_path/target" 在这个仓库里就是错的。
    metadata_json="$(cargo metadata --no-deps --format-version 1 --manifest-path "$sweep_path/Cargo.toml" 2>/dev/null || true)"
    resolved_target_dir="$(printf '%s' "$metadata_json" | grep -o '"target_directory":"[^"]*"' | head -n 1 | sed -E 's/.*:"(.*)"$/\1/')"
    if [ -n "$resolved_target_dir" ]; then
        target_dir="$resolved_target_dir"
    else
        echo "::warning::cargo metadata 未能解析 target_directory,退回猜测路径 $sweep_path/target"
        target_dir="$sweep_path/target"
    fi
fi
min_free_kib="$((auto_min_free_gib * 1024 * 1024))"
max_target_kib="$((auto_max_target_gib * 1024 * 1024))"
auto_maxsize="${auto_max_target_gib}GB"

df_probe_path() {
    local probe="$target_dir"
    while [ ! -e "$probe" ] && [ "$probe" != "/" ]; do
        probe="$(dirname "$probe")"
    done
    echo "$probe"
}

free_kib() {
    df -Pk "$(df_probe_path)" | awk 'NR == 2 { print $4 }'
}

target_kib() {
    if [ -d "$target_dir" ]; then
        du -sk "$target_dir" 2>/dev/null | awk 'NR == 1 { print $1 }'
    else
        echo 0
    fi
}

format_gib() {
    awk -v kib="$1" 'BEGIN { printf "%.1f GiB", kib / 1024 / 1024 }'
}

threshold_exceeded() {
    local current_free_kib current_target_kib
    current_free_kib="$(free_kib)"
    current_target_kib="$(target_kib)"
    [ "$current_free_kib" -lt "$min_free_kib" ] || [ "$current_target_kib" -gt "$max_target_kib" ]
}

report_disk() {
    local label="$1"
    echo "::group::$label"
    df -h "$(df_probe_path)" || true
    if [ -d "$target_dir" ]; then
        du -sh "$target_dir" 2>/dev/null || true
    else
        echo "Target directory not found: $target_dir"
    fi
    echo "Free space: $(format_gib "$(free_kib)")"
    echo "Target size: $(format_gib "$(target_kib)")"
    echo "::endgroup::"
}

if [ -n "${CARGO_SWEEP_BIN:-}" ]; then
    cargo_sweep_cmd=("$CARGO_SWEEP_BIN" sweep)
elif command -v cargo-sweep >/dev/null 2>&1; then
    cargo_sweep_cmd=("$(command -v cargo-sweep)" sweep)
elif cargo sweep --help >/dev/null 2>&1; then
    cargo_sweep_cmd=(cargo sweep)
else
    cargo_sweep_cmd=()
fi

echo "Cargo sweep mode: $mode"
echo "Project path: $sweep_path"
echo "Target directory: $target_dir"
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    echo "Cargo target source: CARGO_TARGET_DIR"
fi
echo "Retention: $retention_days day(s)"
if [ "$mode" = "auto" ]; then
    echo "Auto minimum free disk: ${auto_min_free_gib} GiB"
    echo "Auto target cap: ${auto_max_target_gib} GiB"
elif [ -n "$manual_maxsize" ]; then
    echo "Manual max target size: $manual_maxsize"
fi

report_disk "Disk usage before cargo-sweep"

if [ "${#cargo_sweep_cmd[@]}" -eq 0 ]; then
    echo "cargo-sweep is not installed. Run 'cargo install cargo-sweep --locked' first." >&2
    exit 1
fi

run_sweep() {
    local label="$1"
    shift
    echo "::group::$label"
    printf '+'
    printf ' %q' "${cargo_sweep_cmd[@]}" "$@"
    printf '\n'
    "${cargo_sweep_cmd[@]}" "$@"
    echo "::endgroup::"
}

if [ "$mode" = "dry-run" ]; then
    run_sweep "Run cargo-sweep retention cleanup" --dry-run --time "$retention_days" "$sweep_path"
elif [ "$mode" = "execute" ]; then
    run_sweep "Run cargo-sweep retention cleanup" --time "$retention_days" "$sweep_path"
elif [ "$mode" = "auto" ]; then
    run_sweep "Run cargo-sweep retention cleanup" --time "$retention_days" "$sweep_path"
else
    echo "Unknown cargo sweep mode: $mode" >&2
    exit 2
fi

report_disk "Disk usage after retention cleanup"

if [ "$mode" = "dry-run" ] && [ -n "$manual_maxsize" ]; then
    run_sweep "Run cargo-sweep target-size cleanup" --dry-run --maxsize "$manual_maxsize" "$sweep_path"
    report_disk "Disk usage after target-size dry-run"
elif [ "$mode" = "auto" ] && threshold_exceeded; then
    # 超过阈值时按大小清理最旧的 target 工件,保留最近构建缓存。
    run_sweep "Run cargo-sweep target-size cleanup" --maxsize "$auto_maxsize" "$sweep_path"
    report_disk "Disk usage after target-size cleanup"
elif [ "$mode" = "execute" ] && [ -n "$manual_maxsize" ]; then
    run_sweep "Run cargo-sweep target-size cleanup" --maxsize "$manual_maxsize" "$sweep_path"
    report_disk "Disk usage after target-size cleanup"
fi

if [ "$mode" = "auto" ] && threshold_exceeded; then
    echo "Rust target cleanup finished but disk thresholds are still exceeded." >&2
    echo "Free space: $(format_gib "$(free_kib)") (minimum ${auto_min_free_gib} GiB)" >&2
    echo "Target size: $(format_gib "$(target_kib)") (cap ${auto_max_target_gib} GiB)" >&2
    echo "Run a manual dry-run before retrying: scripts/cleanup-cargo-target.sh --dry-run --maxsize ${auto_maxsize}" >&2
    exit 1
fi
