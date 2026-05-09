use std::{
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tonic::{Request, Response as GrpcResponse, Status};
use uuid::Uuid;

use crate::{
    audit, ok_response,
    proto::rustpanel::v1::{
        cluster_service_server::ClusterService, ClusterNode, DistributeFileRequest,
        DistributeFileResponse, DistributionRecord, HeartbeatClusterNodeRequest,
        HeartbeatClusterNodeResponse, ListClusterNodesRequest, ListClusterNodesResponse,
        ListDistributionRecordsRequest, ListDistributionRecordsResponse, PairClusterNodeRequest,
        PairClusterNodeResponse,
    },
};

const DEFAULT_CLUSTER_ROOT: &str = "/tmp/rustpanel/cluster";
const PAIRING_SECRET_ENV: &str = "RUSTPANEL_CLUSTER_PAIRING_SECRET";

#[derive(Clone, Debug, Default)]
pub struct ClusterServiceImpl;

#[tonic::async_trait]
impl ClusterService for ClusterServiceImpl {
    async fn pair_cluster_node(
        &self,
        request: Request<PairClusterNodeRequest>,
    ) -> Result<GrpcResponse<PairClusterNodeResponse>, Status> {
        let request = request.into_inner();
        validate_pairing_secret(&request.pairing_secret)?;
        let name = sanitize_node_name(&request.name)?;
        let endpoint = validate_endpoint(&request.endpoint)?;
        let mut state = load_state().await?;
        let now = current_timestamp();
        let node_secret = Uuid::new_v4().to_string();
        let stored = StoredClusterNode {
            id: Uuid::new_v4().to_string(),
            name,
            endpoint,
            status: "paired".to_owned(),
            node_secret: node_secret.clone(),
            last_heartbeat_seconds: now,
            created_at_seconds: now,
        };
        let node = stored.clone().into_proto();
        state.nodes.retain(|node| node.name != stored.name);
        state.nodes.push(stored);
        save_state(&state).await?;
        let _ = audit::append_audit_event(
            "cluster",
            "node_paired",
            format!("paired node {}", node.name),
            "local",
        )
        .await;

        Ok(GrpcResponse::new(PairClusterNodeResponse {
            status: Some(ok_response("cluster node paired")),
            node: Some(node),
            node_secret,
        }))
    }

    async fn heartbeat_cluster_node(
        &self,
        request: Request<HeartbeatClusterNodeRequest>,
    ) -> Result<GrpcResponse<HeartbeatClusterNodeResponse>, Status> {
        let request = request.into_inner();
        let mut state = load_state().await?;
        let node = state
            .nodes
            .iter_mut()
            .find(|node| node.id == request.node_id)
            .ok_or_else(|| Status::not_found("cluster node not found"))?;
        if node.node_secret != request.node_secret {
            return Err(Status::permission_denied("invalid node secret"));
        }
        node.status = if request.load_average > 10.0 {
            "degraded".to_owned()
        } else {
            "online".to_owned()
        };
        node.last_heartbeat_seconds = current_timestamp();
        let response_node = node.clone().into_proto();
        save_state(&state).await?;

        Ok(GrpcResponse::new(HeartbeatClusterNodeResponse {
            status: Some(ok_response("heartbeat accepted")),
            node: Some(response_node),
        }))
    }

    async fn list_cluster_nodes(
        &self,
        _request: Request<ListClusterNodesRequest>,
    ) -> Result<GrpcResponse<ListClusterNodesResponse>, Status> {
        let mut nodes = load_state()
            .await?
            .nodes
            .into_iter()
            .map(StoredClusterNode::into_proto)
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.name.cmp(&right.name));

        Ok(GrpcResponse::new(ListClusterNodesResponse {
            status: Some(ok_response("ok")),
            nodes,
        }))
    }

    async fn distribute_file(
        &self,
        request: Request<DistributeFileRequest>,
    ) -> Result<GrpcResponse<DistributeFileResponse>, Status> {
        let request = request.into_inner();
        if request.path.trim().is_empty() {
            return Err(Status::invalid_argument("target path is required"));
        }
        let mut state = load_state().await?;
        let targets = if request.target_node_ids.is_empty() {
            state
                .nodes
                .iter()
                .map(|node| node.id.clone())
                .collect::<Vec<_>>()
        } else {
            request.target_node_ids
        };
        let mut records = Vec::new();
        for node_id in targets {
            let node = state
                .nodes
                .iter()
                .find(|node| node.id == node_id)
                .ok_or_else(|| Status::not_found("target cluster node not found"))?;
            let record =
                distribute_to_node(node, &request.path, &request.content, request.mode).await?;
            state
                .distributions
                .push(StoredDistributionRecord::from_proto(record.clone()));
            records.push(record);
        }
        save_state(&state).await?;
        let _ = audit::append_audit_event(
            "cluster",
            "distribute_file",
            format!("distributed {} to {} node(s)", request.path, records.len()),
            "local",
        )
        .await;

        Ok(GrpcResponse::new(DistributeFileResponse {
            status: Some(ok_response("file distributed")),
            records,
        }))
    }

    async fn list_distribution_records(
        &self,
        request: Request<ListDistributionRecordsRequest>,
    ) -> Result<GrpcResponse<ListDistributionRecordsResponse>, Status> {
        let limit = request.into_inner().limit;
        let limit = if limit == 0 { 100 } else { limit as usize };
        let mut records = load_state()
            .await?
            .distributions
            .into_iter()
            .map(StoredDistributionRecord::into_proto)
            .collect::<Vec<_>>();
        records.sort_by_key(|record| std::cmp::Reverse(record.created_at_seconds));
        records.truncate(limit);

        Ok(GrpcResponse::new(ListDistributionRecordsResponse {
            status: Some(ok_response("ok")),
            records,
        }))
    }
}

async fn distribute_to_node(
    node: &StoredClusterNode,
    path: &str,
    content: &[u8],
    mode: u32,
) -> Result<DistributionRecord, Status> {
    let now = current_timestamp();
    let (status, message) = if node.endpoint.starts_with("file://") || node.endpoint == "local" {
        let inbox = local_node_inbox(&node.id);
        let target = inbox.join(path.trim_start_matches('/'));
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
        }
        tokio::fs::write(&target, content)
            .await
            .map_err(io_status)?;
        #[cfg(unix)]
        if mode > 0 {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))
                .await
                .map_err(io_status)?;
        }
        ("delivered".to_owned(), target.to_string_lossy().to_string())
    } else {
        (
            "queued".to_owned(),
            format!("queued for remote endpoint {}", node.endpoint),
        )
    };

    Ok(DistributionRecord {
        id: Uuid::new_v4().to_string(),
        node_id: node.id.clone(),
        path: path.to_owned(),
        status,
        message,
        created_at_seconds: now,
    })
}

fn validate_pairing_secret(secret: &str) -> Result<(), Status> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(Status::invalid_argument("pairing secret is required"));
    }
    if let Ok(expected) = env::var(PAIRING_SECRET_ENV) {
        if secret != expected {
            return Err(Status::permission_denied("invalid pairing secret"));
        }
    }
    Ok(())
}

fn validate_endpoint(endpoint: &str) -> Result<String, Status> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err(Status::invalid_argument("node endpoint is required"));
    }
    Ok(endpoint.to_owned())
}

fn sanitize_node_name(name: &str) -> Result<String, Status> {
    let sanitized = name
        .trim()
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || char == '-' {
                char.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if sanitized.is_empty() {
        Err(Status::invalid_argument("node name is required"))
    } else {
        Ok(sanitized)
    }
}

async fn load_state() -> Result<StoredClusterState, Status> {
    match tokio::fs::read_to_string(state_path()).await {
        Ok(content) => serde_json::from_str(&content).map_err(io_status),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(StoredClusterState::default())
        }
        Err(error) => Err(io_status(error)),
    }
}

async fn save_state(state: &StoredClusterState) -> Result<(), Status> {
    tokio::fs::create_dir_all(cluster_root())
        .await
        .map_err(io_status)?;
    let content = serde_json::to_string_pretty(state).map_err(io_status)?;
    tokio::fs::write(state_path(), content)
        .await
        .map_err(io_status)
}

fn cluster_root() -> PathBuf {
    env::var("RUSTPANEL_CLUSTER_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CLUSTER_ROOT))
}

fn state_path() -> PathBuf {
    cluster_root().join("state.json")
}

fn local_node_inbox(node_id: &str) -> PathBuf {
    cluster_root().join("inbox").join(node_id)
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredClusterState {
    #[serde(default)]
    nodes: Vec<StoredClusterNode>,
    #[serde(default)]
    distributions: Vec<StoredDistributionRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredClusterNode {
    id: String,
    name: String,
    endpoint: String,
    status: String,
    node_secret: String,
    last_heartbeat_seconds: u64,
    created_at_seconds: u64,
}

impl StoredClusterNode {
    fn into_proto(self) -> ClusterNode {
        ClusterNode {
            id: self.id,
            name: self.name,
            endpoint: self.endpoint,
            status: self.status,
            last_heartbeat_seconds: self.last_heartbeat_seconds,
            created_at_seconds: self.created_at_seconds,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredDistributionRecord {
    id: String,
    node_id: String,
    path: String,
    status: String,
    message: String,
    created_at_seconds: u64,
}

impl StoredDistributionRecord {
    fn from_proto(record: DistributionRecord) -> Self {
        Self {
            id: record.id,
            node_id: record.node_id,
            path: record.path,
            status: record.status,
            message: record.message,
            created_at_seconds: record.created_at_seconds,
        }
    }

    fn into_proto(self) -> DistributionRecord {
        DistributionRecord {
            id: self.id,
            node_id: self.node_id,
            path: self.path,
            status: self.status,
            message: self.message,
            created_at_seconds: self.created_at_seconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn sanitizes_node_names() {
        assert_eq!(sanitize_node_name("Node A_1").expect("name"), "node-a-1");
    }

    #[test]
    fn validates_empty_pairing_secret() {
        assert!(validate_pairing_secret("").is_err());
    }

    #[test]
    fn local_inbox_uses_node_id() {
        let path = local_node_inbox("node-1");

        assert!(path.ends_with(Path::new("inbox").join("node-1")));
    }
}
