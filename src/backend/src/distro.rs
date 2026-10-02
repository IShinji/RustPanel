//! `/etc/os-release` 解析——给 PackageManager(apt/apk/...)挑对应后端用。
//!
//! `appstore.rs` 的 nginx-mainline preinstall shell 脚本里已经有一份等价的
//! os-release 解析逻辑;shell 和 Rust 没法共享代码,但取字段的方式保持一致:
//! 先看 `ID`,认不出来再按 `ID_LIKE` 里认识的词退。
//!
//! 这次只识别 Debian 系(apt)和 Alpine(apk)——RHEL 系(dnf + SELinux +
//! firewalld)和 Arch(pacman)按计划留给后续独立项目,这里先归到 `Unknown`。

use std::{collections::HashMap, env, path::PathBuf, sync::OnceLock};

/// 识别出的发行版家族,决定走哪个包管理器后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distro {
    /// Debian / Ubuntu 及其衍生版,apt-get。
    Debian,
    /// Alpine,apk。
    Alpine,
    /// 识别出了 `ID`/`ID_LIKE` 但还没做对应后端,或者读不到/解析不出来。
    Unknown,
}

/// `RUSTPANEL_OS_RELEASE_PATH` 覆盖要读的文件路径,给测试用。
fn os_release_path() -> PathBuf {
    env::var("RUSTPANEL_OS_RELEASE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/os-release"))
}

fn parse_os_release(text: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_owned();
        fields.insert(key.trim().to_owned(), value);
    }
    fields
}

fn classify(fields: &HashMap<String, String>) -> Distro {
    let mut candidates = Vec::new();
    if let Some(id) = fields.get("ID") {
        candidates.push(id.clone());
    }
    if let Some(like) = fields.get("ID_LIKE") {
        candidates.extend(like.split_whitespace().map(str::to_owned));
    }
    for candidate in candidates {
        match candidate.as_str() {
            "debian" | "ubuntu" => return Distro::Debian,
            "alpine" => return Distro::Alpine,
            _ => {}
        }
    }
    Distro::Unknown
}

fn detect() -> Distro {
    let Ok(text) = std::fs::read_to_string(os_release_path()) else {
        return Distro::Unknown;
    };
    classify(&parse_os_release(&text))
}

/// 不缓存的探测,给测试用(同一进程里 `current()` 只探测一次)。
#[cfg(test)]
fn detect_uncached() -> Distro {
    detect()
}

pub fn current() -> Distro {
    static INIT: OnceLock<Distro> = OnceLock::new();
    *INIT.get_or_init(detect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_release(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("tmp file");
        file.write_all(contents.as_bytes()).expect("write");
        file
    }

    #[test]
    fn classifies_debian_ubuntu_alpine_and_unknown() {
        let debian = write_release("ID=debian\nVERSION_CODENAME=bookworm\n");
        env::set_var("RUSTPANEL_OS_RELEASE_PATH", debian.path());
        assert_eq!(detect_uncached(), Distro::Debian);

        let ubuntu = write_release("ID=ubuntu\nID_LIKE=debian\nUBUNTU_CODENAME=noble\n");
        env::set_var("RUSTPANEL_OS_RELEASE_PATH", ubuntu.path());
        assert_eq!(detect_uncached(), Distro::Debian);

        let alpine = write_release("ID=alpine\nVERSION_ID=3.24.0\n");
        env::set_var("RUSTPANEL_OS_RELEASE_PATH", alpine.path());
        assert_eq!(detect_uncached(), Distro::Alpine);

        // Rocky 之类有 ID_LIKE 但这次没做对应后端,归 Unknown 而不是瞎猜。
        let rocky = write_release("ID=rocky\nID_LIKE=\"rhel centos fedora\"\n");
        env::set_var("RUSTPANEL_OS_RELEASE_PATH", rocky.path());
        assert_eq!(detect_uncached(), Distro::Unknown);

        env::remove_var("RUSTPANEL_OS_RELEASE_PATH");
    }

    #[test]
    fn missing_file_is_unknown_not_a_panic() {
        env::set_var("RUSTPANEL_OS_RELEASE_PATH", "/definitely/does/not/exist");
        assert_eq!(detect_uncached(), Distro::Unknown);
        env::remove_var("RUSTPANEL_OS_RELEASE_PATH");
    }

    #[test]
    fn current_is_cached_across_calls() {
        assert_eq!(current(), current());
    }
}
