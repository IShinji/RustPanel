//! 站点关联的 systemd 单元与日志文件(类似宝塔的「项目管理」)。
//!
//! 场景:应用自己带 systemd 服务(比如 jobwatch-serve.service + 抓取 timer),
//! 站点只负责域名 / 反代 / 证书。把单元和日志文件挂到站点上以后,面板里就能
//! 看运行状态、启停重启、看日志尾部,不用每次开终端。
//!
//! 安全边界:只能操作**已关联到该站点**的单元和日志文件;关联本身是写操作,
//! readonly 角色做不了(RBAC 按方法名前缀判定,Update/Control 不是只读)。

use std::{
    io::{Read, Seek, SeekFrom},
    path::{Component, Path},
};

use tonic::Status;

use crate::proto::rustpanel::v1::{SiteItem, SiteServiceAction, SiteServiceStatus};

pub(super) const MAX_LINKED: usize = 16;
const DEFAULT_LOG_LINES: u32 = 200;
const MAX_LOG_LINES: u32 = 1000;
/// 单次日志响应上限:从文件尾部最多读这么多字节(不整文件进内存)。
const MAX_LOG_BYTES: u64 = 64 * 1024;

/// 单元名:只允许 systemd 合法字符,且必须是 .service / .timer。
pub(super) fn validate_unit(unit: &str) -> Result<(), Status> {
    let valid_chars = unit
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '@' | '.' | '_' | '-' | ':'));
    let valid_suffix = unit.ends_with(".service") || unit.ends_with(".timer");
    if unit.is_empty() || unit.len() > 128 || !valid_chars || !valid_suffix || unit.starts_with('-')
    {
        return Err(Status::invalid_argument(format!(
            "无效的单元名 `{unit}`:只支持 *.service / *.timer"
        )));
    }
    Ok(())
}

/// 日志路径:绝对路径、不含 `..`。
pub(super) fn validate_log_path(path: &str) -> Result<(), Status> {
    let parsed = Path::new(path);
    if !parsed.is_absolute()
        || parsed
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || path.len() > 512
    {
        return Err(Status::invalid_argument(format!(
            "无效的日志路径 `{path}`:需要不含 .. 的绝对路径"
        )));
    }
    Ok(())
}

/// 规整用户输入:去空白、去重、校验、限量。
pub(super) fn normalize_links(
    units: Vec<String>,
    log_paths: Vec<String>,
) -> Result<(Vec<String>, Vec<String>), Status> {
    let mut clean_units = Vec::new();
    for unit in units {
        let unit = unit.trim().to_owned();
        if unit.is_empty() || clean_units.contains(&unit) {
            continue;
        }
        validate_unit(&unit)?;
        clean_units.push(unit);
    }
    let mut clean_paths = Vec::new();
    for path in log_paths {
        let path = path.trim().to_owned();
        if path.is_empty() || clean_paths.contains(&path) {
            continue;
        }
        validate_log_path(&path)?;
        clean_paths.push(path);
    }
    if clean_units.len() > MAX_LINKED || clean_paths.len() > MAX_LINKED {
        return Err(Status::invalid_argument(format!(
            "每个站点最多关联 {MAX_LINKED} 个单元和 {MAX_LINKED} 个日志文件"
        )));
    }
    Ok((clean_units, clean_paths))
}

pub(super) fn ensure_linked_unit(site: &SiteItem, unit: &str) -> Result<(), Status> {
    if site.service_units.iter().any(|linked| linked == unit) {
        Ok(())
    } else {
        Err(Status::permission_denied(format!(
            "单元 `{unit}` 未关联到站点 {}",
            site.name
        )))
    }
}

const SHOW_PROPERTIES: &str = "Description,ActiveState,SubState,UnitFileState,MainPID,\
ActiveEnterTimestamp,Result,NextElapseUSecRealtime,LastTriggerUSec,LoadState";

pub(super) async fn unit_status(unit: &str) -> SiteServiceStatus {
    let mut status = SiteServiceStatus {
        unit: unit.to_owned(),
        ..Default::default()
    };
    let output = tokio::process::Command::new("systemctl")
        .args(["show", unit, "--no-pager", "-p", SHOW_PROPERTIES])
        .output()
        .await;
    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            status.error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return status;
        }
        Err(error) => {
            status.error = format!("systemctl 不可用: {error}");
            return status;
        }
    };
    apply_show_output(&mut status, &String::from_utf8_lossy(&output.stdout));
    if status.main_pid > 0 {
        status.memory_rss_bytes = process_rss_bytes(status.main_pid).unwrap_or(0);
    }
    status
}

fn apply_show_output(status: &mut SiteServiceStatus, text: &str) {
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().to_owned();
        match key {
            "Description" => status.description = value,
            "ActiveState" => status.active_state = value,
            "SubState" => status.sub_state = value,
            "UnitFileState" => status.unit_file_state = value,
            "MainPID" => status.main_pid = value.parse().unwrap_or(0),
            "ActiveEnterTimestamp" => status.active_since = value,
            "Result" => status.result = value,
            "NextElapseUSecRealtime" => status.next_trigger = value,
            "LastTriggerUSec" => status.last_trigger = value,
            "LoadState" if value == "not-found" => {
                status.error = "单元不存在".to_owned();
            }
            _ => {}
        }
    }
}

/// OpenVZ 等环境 systemd 的 MemoryCurrent 常是 [not set],直接读 /proc 更可靠。
fn process_rss_bytes(pid: u32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_vm_rss(&text)
}

fn parse_vm_rss(status: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

pub(super) async fn control_unit(unit: &str, action: SiteServiceAction) -> Result<(), Status> {
    let verb = match action {
        SiteServiceAction::Start => "start",
        SiteServiceAction::Stop => "stop",
        SiteServiceAction::Restart => "restart",
        SiteServiceAction::Unspecified => {
            return Err(Status::invalid_argument("action is required"));
        }
    };
    // oneshot 服务(比如抓取任务)start 会阻塞到跑完;--no-block 让面板立刻返回,
    // 状态由前端轮询 GetSiteServices 看。
    let output = tokio::process::Command::new("systemctl")
        .args([verb, "--no-block", unit])
        .output()
        .await
        .map_err(|error| Status::unavailable(format!("systemctl 不可用: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Status::failed_precondition(format!(
            "systemctl {verb} {unit} 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

pub(super) fn clamp_lines(lines: u32) -> u32 {
    if lines == 0 {
        DEFAULT_LOG_LINES
    } else {
        lines.min(MAX_LOG_LINES)
    }
}

/// journal 里某单元的最后 N 行。
pub(super) async fn journal_tail(unit: &str, lines: u32) -> Result<(String, bool), Status> {
    let output = tokio::process::Command::new("journalctl")
        .args(["-u", unit, "--no-pager", "-o", "short-iso", "-n"])
        .arg(lines.to_string())
        .output()
        .await
        .map_err(|error| Status::unavailable(format!("journalctl 不可用: {error}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(cap_tail(&text))
}

/// 文件最后 N 行:只从尾部 seek 读最多 MAX_LOG_BYTES,大日志也不整文件进内存。
pub(super) async fn file_tail(path: &str, lines: u32) -> Result<(String, bool), Status> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || read_file_tail(Path::new(&path), lines))
        .await
        .map_err(|error| Status::internal(error.to_string()))?
}

fn read_file_tail(path: &Path, lines: u32) -> Result<(String, bool), Status> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| Status::not_found(format!("打不开日志 {}: {error}", path.display())))?;
    let len = file
        .metadata()
        .map_err(|error| Status::internal(error.to_string()))?
        .len();
    let start = len.saturating_sub(MAX_LOG_BYTES);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| Status::internal(error.to_string()))?;
    let mut buffer = Vec::new();
    file.take(MAX_LOG_BYTES)
        .read_to_end(&mut buffer)
        .map_err(|error| Status::internal(error.to_string()))?;
    let text = String::from_utf8_lossy(&buffer);
    // 从中间截断时第一行多半是半行,丢掉
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Ok(last_lines(text, lines, start > 0))
}

fn last_lines(text: &str, lines: u32, already_truncated: bool) -> (String, bool) {
    let all: Vec<&str> = text.lines().collect();
    let keep = usize::try_from(lines).unwrap_or(usize::MAX).min(all.len());
    let truncated = already_truncated || keep < all.len();
    (all[all.len() - keep..].join("\n"), truncated)
}

fn cap_tail(text: &str) -> (String, bool) {
    let max = usize::try_from(MAX_LOG_BYTES).unwrap_or(usize::MAX);
    if text.len() <= max {
        return (text.to_owned(), false);
    }
    let mut cut = text.len() - max;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    (text[cut..].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_names_are_restricted_to_services_and_timers() {
        assert!(validate_unit("jobwatch-serve.service").is_ok());
        assert!(validate_unit("jobwatch-collect.timer").is_ok());
        assert!(validate_unit("sws@rustpanel.service").is_ok());
        assert!(validate_unit("jobwatch").is_err());
        assert!(validate_unit("foo.socket").is_err());
        assert!(validate_unit("a b.service").is_err());
        assert!(validate_unit("x.service;rm -rf /").is_err());
        assert!(validate_unit("--help.service").is_err());
    }

    #[test]
    fn log_paths_must_be_absolute_without_parent_dirs() {
        assert!(validate_log_path("/opt/jobwatch/data/log.txt").is_ok());
        assert!(validate_log_path("data/log.txt").is_err());
        assert!(validate_log_path("/opt/../etc/shadow").is_err());
    }

    #[test]
    fn normalize_dedupes_and_trims() {
        let (units, paths) = normalize_links(
            vec![
                " jobwatch-serve.service ".to_owned(),
                "jobwatch-serve.service".to_owned(),
                String::new(),
            ],
            vec!["/var/log/a.log".to_owned(), "/var/log/a.log".to_owned()],
        )
        .expect("normalize");
        assert_eq!(units, vec!["jobwatch-serve.service"]);
        assert_eq!(paths, vec!["/var/log/a.log"]);
    }

    #[test]
    fn only_linked_units_can_be_controlled() {
        let site = SiteItem {
            name: "jobs".to_owned(),
            service_units: vec!["jobwatch-serve.service".to_owned()],
            ..Default::default()
        };
        assert!(ensure_linked_unit(&site, "jobwatch-serve.service").is_ok());
        assert_eq!(
            ensure_linked_unit(&site, "sshd.service")
                .expect_err("unlinked")
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn parses_systemctl_show_output() {
        let mut status = SiteServiceStatus::default();
        apply_show_output(
            &mut status,
            "Description=jobwatch: 看板\nActiveState=active\nSubState=running\n\
             UnitFileState=enabled\nMainPID=123\nResult=success\n\
             ActiveEnterTimestamp=Fri 2026-09-25 05:56:00 CEST\nLoadState=loaded\n",
        );
        assert_eq!(status.active_state, "active");
        assert_eq!(status.sub_state, "running");
        assert_eq!(status.main_pid, 123);
        assert_eq!(status.description, "jobwatch: 看板");
        assert!(status.error.is_empty());

        let mut missing = SiteServiceStatus::default();
        apply_show_output(&mut missing, "LoadState=not-found\nActiveState=inactive\n");
        assert_eq!(missing.error, "单元不存在");
    }

    #[test]
    fn parses_vm_rss() {
        assert_eq!(
            parse_vm_rss("Name:\tjobwatch\nVmRSS:\t    3848 kB\n"),
            Some(3848 * 1024)
        );
        assert_eq!(parse_vm_rss("Name:\tx\n"), None);
    }

    #[test]
    fn file_tail_reads_only_the_end_of_large_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("log.txt");
        let mut content = String::new();
        for i in 0..20_000 {
            content.push_str(&format!("line {i}\n"));
        }
        std::fs::write(&path, &content).expect("write");

        let (tail, truncated) = read_file_tail(&path, 3).expect("tail");
        assert_eq!(tail, "line 19997\nline 19998\nline 19999");
        assert!(truncated);

        let small = dir.path().join("small.txt");
        std::fs::write(&small, "a\nb\n").expect("write small");
        let (tail, truncated) = read_file_tail(&small, 10).expect("tail small");
        assert_eq!(tail, "a\nb");
        assert!(!truncated);
    }

    #[test]
    fn clamp_lines_defaults_and_caps() {
        assert_eq!(clamp_lines(0), 200);
        assert_eq!(clamp_lines(50), 50);
        assert_eq!(clamp_lines(99_999), 1000);
    }
}
