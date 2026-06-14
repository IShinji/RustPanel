use std::{env, path::Path};

use sysinfo::{Disks, System};
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        toolbox_service_server::ToolboxService, CreateSwapRequest, CreateSwapResponse,
        GetToolboxRequest, GetToolboxResponse, SetTimezoneRequest, SetTimezoneResponse,
    },
};

// 系统改动(建 swap / 改时区)默认不执行,设 RUSTPANEL_TOOLBOX_APPLY=1 才真动手。
// 与防火墙 apply 同思路:开发/CI/未授权环境不应被这些破坏性操作影响。
const APPLY_ENV: &str = "RUSTPANEL_TOOLBOX_APPLY";

fn should_apply() -> bool {
    env::var(APPLY_ENV).is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

#[derive(Clone, Debug, Default)]
pub struct ToolboxServiceImpl;

#[tonic::async_trait]
impl ToolboxService for ToolboxServiceImpl {
    async fn get_toolbox(
        &self,
        _request: Request<GetToolboxRequest>,
    ) -> Result<GrpcResponse<GetToolboxResponse>, Status> {
        let mut system = System::new();
        system.refresh_memory();
        Ok(GrpcResponse::new(GetToolboxResponse {
            status: Some(ok_response("ok")),
            swap_total_bytes: system.total_swap(),
            swap_used_bytes: system.used_swap(),
            timezone: read_timezone(),
            root_available_bytes: root_available_bytes(),
        }))
    }

    async fn create_swap(
        &self,
        request: Request<CreateSwapRequest>,
    ) -> Result<GrpcResponse<CreateSwapResponse>, Status> {
        let size_mb = request.into_inner().size_mb;
        if !(64..=4096).contains(&size_mb) {
            return Err(Status::invalid_argument("swap size must be 64..=4096 MB"));
        }
        if !should_apply() {
            return Err(Status::failed_precondition(
                "系统应用未启用(设 RUSTPANEL_TOOLBOX_APPLY=1 后重试)",
            ));
        }
        let path = env::var("RUSTPANEL_SWAP_PATH").unwrap_or_else(|_| "/swapfile".to_owned());
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Err(Status::already_exists(format!("{path} already exists")));
        }
        // 留 200MB 余量,别把根分区写满。
        let need = u64::from(size_mb) * 1024 * 1024 + 200 * 1024 * 1024;
        if root_available_bytes() < need {
            return Err(Status::failed_precondition(
                "根分区可用空间不足以创建该 swap",
            ));
        }
        // size_mb 已校验为整数,path 来自运维 env,sh -c 拼接安全。
        let script = format!(
            "set -e; fallocate -l {size_mb}M {path} || dd if=/dev/zero of={path} bs=1M count={size_mb}; \
             chmod 600 {path}; mkswap {path}; swapon {path}; \
             grep -q '{path}' /etc/fstab || echo '{path} none swap sw 0 0' >> /etc/fstab"
        );
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .output()
            .await
            .map_err(io_status)?;
        if !output.status.success() {
            return Err(Status::internal(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        Ok(GrpcResponse::new(CreateSwapResponse {
            status: Some(ok_response(format!("swap {size_mb}MB created at {path}"))),
        }))
    }

    async fn set_timezone(
        &self,
        request: Request<SetTimezoneRequest>,
    ) -> Result<GrpcResponse<SetTimezoneResponse>, Status> {
        let tz = request.into_inner().timezone.trim().to_owned();
        if tz.is_empty()
            || !tz
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
        {
            return Err(Status::invalid_argument("invalid timezone"));
        }
        let zoneinfo = format!("/usr/share/zoneinfo/{tz}");
        if !tokio::fs::try_exists(&zoneinfo).await.unwrap_or(false) {
            return Err(Status::invalid_argument(format!("unknown timezone: {tz}")));
        }
        if !should_apply() {
            return Err(Status::failed_precondition(
                "系统应用未启用(设 RUSTPANEL_TOOLBOX_APPLY=1 后重试)",
            ));
        }
        tokio::fs::write("/etc/timezone", format!("{tz}\n"))
            .await
            .map_err(io_status)?;
        #[cfg(unix)]
        {
            let _ = tokio::fs::remove_file("/etc/localtime").await;
            tokio::fs::symlink(&zoneinfo, "/etc/localtime")
                .await
                .map_err(io_status)?;
        }
        Ok(GrpcResponse::new(SetTimezoneResponse {
            status: Some(ok_response(format!("timezone set to {tz}"))),
        }))
    }
}

fn root_available_bytes() -> u64 {
    Disks::new_with_refreshed_list()
        .iter()
        .find(|disk| disk.mount_point() == Path::new("/"))
        .map(|disk| disk.available_space())
        .unwrap_or(0)
}

fn read_timezone() -> String {
    if let Ok(content) = std::fs::read_to_string("/etc/timezone") {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    if let Ok(link) = std::fs::read_link("/etc/localtime") {
        let text = link.to_string_lossy();
        if let Some(index) = text.find("zoneinfo/") {
            return text[index + "zoneinfo/".len()..].to_owned();
        }
    }
    "UTC".to_owned()
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}
