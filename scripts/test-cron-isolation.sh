#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
test_dir="$(mktemp -d)"
trap 'rm -rf "$test_dir"' EXIT
export RUSTPANEL_CRON_D_DIR="$test_dir/cron.d"
source "$root_dir/deploy/systemd-units.sh"

# 用函数替代宿主命令,任何误访问都记录下来,绝不读写真实 crontab。
crontab() {
  printf '%s\n' "$*" >> "$test_dir/crontab-calls"
  if [[ "$1" == - ]]; then
    cat > "$test_dir/crontab-content"
  elif [[ -f "$test_dir/crontab-content" ]]; then
    cat "$test_dir/crontab-content"
  fi
}
uname() { printf '%s\n' "${test_os:-Linux}"; }
id() { printf '%s\n' "${test_uid:-0}"; }

# 自定义目录缺失时,即使模拟 Linux root 也不能回退。
if rustpanel_cron_set rustpanel-alerts '*/15 * * * *' true; then
  echo 'missing isolated directory unexpectedly accepted' >&2
  exit 1
fi
rustpanel_cron_unset rustpanel-alerts
[[ ! -e "$test_dir/crontab-calls" ]]

mkdir "$RUSTPANEL_CRON_D_DIR"
rustpanel_cron_set rustpanel-alerts '*/15 * * * *' true
grep -q '^\*/15 \* \* \* \* root true$' "$RUSTPANEL_CRON_D_DIR/rustpanel-alerts"
rustpanel_cron_unset rustpanel-alerts
[[ ! -e "$RUSTPANEL_CRON_D_DIR/rustpanel-alerts" ]]
[[ ! -e "$test_dir/crontab-calls" ]]

# 在子 shell 中隔离目录探测,让各宿主都能验证正式回退条件。
for scenario in macos user root; do
  (
    RUSTPANEL_CRON_D_DIR=/etc/cron.d
    test_os=Linux test_uid=0
    [[ "$scenario" != macos ]] || test_os=Darwin
    [[ "$scenario" != user ]] || test_uid=501
    # cron.d 路径存在与否由测试替身控制,其他条件保留 Bash 原语义。
    eval "$(declare -f rustpanel_cron_set rustpanel_cron_unset | sed 's/\[\[ -d "\$RUSTPANEL_CRON_D_DIR" \]\]/false/g')"
    if [[ "$scenario" == root ]]; then
      printf '%s\n' '0 1 * * * user-task' > "$test_dir/crontab-content"
      rustpanel_cron_set rustpanel-alerts '*/15 * * * *' true
      [[ -s "$test_dir/crontab-calls" ]]
      grep -q '# rustpanel:rustpanel-alerts begin' "$test_dir/crontab-content"
      rustpanel_cron_unset rustpanel-alerts
      [[ "$(cat "$test_dir/crontab-content")" == '0 1 * * * user-task' ]]
    else
      if rustpanel_cron_set rustpanel-alerts '*/15 * * * *' true; then exit 1; fi
      rustpanel_cron_unset rustpanel-alerts
      [[ ! -e "$test_dir/crontab-calls" ]]
    fi
  )
done
echo 'cron isolation tests passed'
