use std::pin::Pin;

use bollard::{
    container::{
        ListContainersOptions, LogsOptions, RemoveContainerOptions, RestartContainerOptions,
        StartContainerOptions, StopContainerOptions,
    },
    image::CreateImageOptions,
    secret::{ContainerSummary, Port},
    Docker,
};
use futures_core::Stream;
use futures_util::StreamExt;
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        docker_service_server::DockerService, ContainerItem, ContainerPort, ListContainersRequest,
        ListContainersResponse, PauseContainerRequest, PauseContainerResponse, PullImageRequest,
        PullImageResponse, RemoveContainerRequest, RemoveContainerResponse,
        RestartContainerRequest, RestartContainerResponse, StartContainerRequest,
        StartContainerResponse, StopContainerRequest, StopContainerResponse,
        WatchContainerLogsRequest, WatchContainerLogsResponse,
    },
};

#[derive(Clone, Debug, Default)]
pub struct DockerServiceImpl;

#[tonic::async_trait]
impl DockerService for DockerServiceImpl {
    type WatchContainerLogsStream =
        Pin<Box<dyn Stream<Item = Result<WatchContainerLogsResponse, Status>> + Send>>;

    async fn list_containers(
        &self,
        request: Request<ListContainersRequest>,
    ) -> Result<GrpcResponse<ListContainersResponse>, Status> {
        let docker = docker_client()?;
        let containers = docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: request.into_inner().all,
                ..Default::default()
            }))
            .await
            .map_err(docker_status)?
            .into_iter()
            .map(container_summary)
            .collect::<Vec<_>>();

        Ok(GrpcResponse::new(ListContainersResponse {
            status: Some(ok_response("ok")),
            containers,
        }))
    }

    async fn start_container(
        &self,
        request: Request<StartContainerRequest>,
    ) -> Result<GrpcResponse<StartContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .start_container(
                &request.into_inner().container_id,
                None::<StartContainerOptions<String>>,
            )
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(StartContainerResponse {
            status: Some(ok_response("container started")),
        }))
    }

    async fn stop_container(
        &self,
        request: Request<StopContainerRequest>,
    ) -> Result<GrpcResponse<StopContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .stop_container(
                &request.into_inner().container_id,
                None::<StopContainerOptions>,
            )
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(StopContainerResponse {
            status: Some(ok_response("container stopped")),
        }))
    }

    async fn restart_container(
        &self,
        request: Request<RestartContainerRequest>,
    ) -> Result<GrpcResponse<RestartContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .restart_container(
                &request.into_inner().container_id,
                None::<RestartContainerOptions>,
            )
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(RestartContainerResponse {
            status: Some(ok_response("container restarted")),
        }))
    }

    async fn remove_container(
        &self,
        request: Request<RemoveContainerRequest>,
    ) -> Result<GrpcResponse<RemoveContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .remove_container(
                &request.into_inner().container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(RemoveContainerResponse {
            status: Some(ok_response("container removed")),
        }))
    }

    async fn pause_container(
        &self,
        request: Request<PauseContainerRequest>,
    ) -> Result<GrpcResponse<PauseContainerResponse>, Status> {
        let docker = docker_client()?;
        docker
            .pause_container(&request.into_inner().container_id)
            .await
            .map_err(docker_status)?;

        Ok(GrpcResponse::new(PauseContainerResponse {
            status: Some(ok_response("container paused")),
        }))
    }

    async fn pull_image(
        &self,
        request: Request<PullImageRequest>,
    ) -> Result<GrpcResponse<PullImageResponse>, Status> {
        let request = request.into_inner();
        let docker = docker_client()?;
        let mut stream = docker.create_image(
            Some(CreateImageOptions {
                from_image: request.image,
                tag: request.tag,
                ..Default::default()
            }),
            None,
            None,
        );

        while let Some(event) = stream.next().await {
            event.map_err(docker_status)?;
        }

        Ok(GrpcResponse::new(PullImageResponse {
            status: Some(ok_response("image pulled")),
        }))
    }

    async fn watch_container_logs(
        &self,
        request: Request<WatchContainerLogsRequest>,
    ) -> Result<GrpcResponse<Self::WatchContainerLogsStream>, Status> {
        let request = request.into_inner();
        let docker = docker_client()?;
        let tail = request.tail_lines.max(100).to_string();
        let stream = docker
            .logs(
                &request.container_id,
                Some(LogsOptions::<String> {
                    follow: true,
                    stdout: true,
                    stderr: true,
                    tail,
                    ..Default::default()
                }),
            )
            .map(|event| {
                event
                    .map(|output| WatchContainerLogsResponse {
                        status: Some(ok_response("ok")),
                        line: log_output_text(output),
                    })
                    .map_err(docker_status)
            });

        Ok(GrpcResponse::new(Box::pin(stream)))
    }
}

fn docker_client() -> Result<Docker, Status> {
    Docker::connect_with_local_defaults().map_err(docker_status)
}

fn docker_status(error: impl std::fmt::Display) -> Status {
    Status::unavailable(error.to_string())
}

fn container_summary(summary: ContainerSummary) -> ContainerItem {
    ContainerItem {
        id: summary.id.unwrap_or_default(),
        name: summary
            .names
            .and_then(|names| names.into_iter().next())
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_owned(),
        image: summary.image.unwrap_or_default(),
        state: summary.state.unwrap_or_default(),
        status_text: summary.status.unwrap_or_default(),
        ports: summary
            .ports
            .unwrap_or_default()
            .into_iter()
            .map(container_port)
            .collect(),
        created_at_seconds: summary.created.unwrap_or_default(),
    }
}

fn container_port(port: Port) -> ContainerPort {
    ContainerPort {
        ip: port.ip.unwrap_or_default(),
        private_port: u32::from(port.private_port),
        public_port: port.public_port.map(u32::from).unwrap_or_default(),
        r#type: port
            .typ
            .map(|port_type| format!("{port_type:?}").to_ascii_lowercase())
            .unwrap_or_default(),
    }
}

fn log_output_text(output: bollard::container::LogOutput) -> String {
    match output {
        bollard::container::LogOutput::StdOut { message }
        | bollard::container::LogOutput::StdErr { message }
        | bollard::container::LogOutput::StdIn { message }
        | bollard::container::LogOutput::Console { message } => {
            String::from_utf8_lossy(&message).to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_container_port_defaults() {
        let port = container_port(Port {
            ip: Some("127.0.0.1".to_owned()),
            private_port: 80,
            public_port: Some(8080),
            typ: None,
        });

        assert_eq!(port.private_port, 80);
        assert_eq!(port.public_port, 8080);
        assert_eq!(port.r#type, "");
    }
}
