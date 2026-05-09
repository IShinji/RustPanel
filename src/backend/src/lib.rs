pub mod proto {
    pub mod rustpanel {
        pub mod v1 {
            tonic::include_proto!("rustpanel.v1");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::proto::rustpanel::v1::{HealthCheckResponse, HealthStatus, Response};

    #[test]
    fn generated_system_contract_is_usable() {
        let response = HealthCheckResponse {
            status: Some(Response {
                code: 0,
                message: "ok".to_owned(),
                data: None,
            }),
            health: HealthStatus::Serving.into(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };

        assert_eq!(response.status.expect("status").code, 0);
        assert_eq!(response.health, HealthStatus::Serving as i32);
    }
}
