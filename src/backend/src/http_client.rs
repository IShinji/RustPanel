//! 统一的出站 HTTP 客户端 TLS 配置。
//!
//! reqwest 0.13 把证书验证默认换成了 `rustls-platform-verifier`——在 Linux 上
//! 靠系统的 `ca-certificates` 包去发现信任链(见该 crate 文档:Linux 没有
//! 统一的"平台验证 API",退化成读系统 CA bundle)。这跟项目"低配主机、尽量不
//! 依赖系统自带东西"的方针冲突:精简过的 Alpine/OpenVZ micro 镜像很可能没装
//! `ca-certificates`,会导致所有出站 HTTPS(备份上传、DNS API、通知渠道)因为
//! 证书验证失败而挂掉。
//!
//! 这里完全绕开 `rustls-platform-verifier`,手动拿 `webpki-roots` 内置的
//! Mozilla 根证书列表构造标准 `rustls::RootCertStore`——跟升级前 reqwest 内置
//! 的 `rustls-tls` feature 默认行为完全等价,不依赖系统证书包。

use std::sync::OnceLock;

use rustls::ClientConfig;

fn template() -> &'static ClientConfig {
    static TEMPLATE: OnceLock<ClientConfig> = OnceLock::new();
    TEMPLATE.get_or_init(|| {
        let mut root_store = rustls::RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    })
}

/// 给 `reqwest::ClientBuilder` 接入上面这份不依赖系统证书包的 TLS 配置。
/// 每个要发起出站 HTTPS 请求的调用点,原来 `reqwest::Client::new()` /
/// `reqwest::Client::builder()` 的地方都改走这个函数,其余配置(timeout 等)
/// 照常链式往下加,最后再 `.build()`。
pub fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().use_preconfigured_tls(template().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_builder_produces_a_usable_client() {
        // 只验证不 panic、能正常 build 出 Client——真实网络请求留给其它模块
        // 已有的集成式用法覆盖,这里不引入网络依赖的测试。
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        assert!(client_builder().build().is_ok());
    }
}
