//! 面板来源 IP 白名单(可选)。
//!
//! NAT 小鸡上面板端口往往直接暴露在公网 IP,即使前面挂了 Cloudflare,别人也能绕过它
//! 直连 IP:端口。设 `RUSTPANEL_PANEL_TRUSTED_SOURCES=cloudflare` 后,只接受 Cloudflare
//! 回源网段、本机回环,以及 `RUSTPANEL_PANEL_ALLOW_CIDRS`(逗号分隔)里额外放行的网段;
//! 其余连接一律 403。默认关闭。
//!
//! 过滤发生在连接层(axum::serve 的 make-service),被拒的请求不进业务逻辑,
//! 也不会计入节俭模式的活动(扫描器不会把休眠的面板吵醒后一直占着)。

use std::{
    convert::Infallible,
    future::{ready, Ready},
    net::IpAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    body::Body,
    extract::Request,
    http::{Response as HttpResponse, StatusCode},
    serve::IncomingStream,
};
use tower::{Service, ServiceExt};

/// Cloudflare 公布的回源网段(https://www.cloudflare.com/ips/)。
const CLOUDFLARE_RANGES: &[&str] = &[
    "173.245.48.0/20",
    "103.21.244.0/22",
    "103.22.200.0/22",
    "103.31.4.0/22",
    "141.101.64.0/18",
    "108.162.192.0/18",
    "190.93.240.0/20",
    "188.114.96.0/20",
    "197.234.240.0/22",
    "198.41.128.0/17",
    "162.158.0.0/15",
    "104.16.0.0/13",
    "104.24.0.0/14",
    "172.64.0.0/13",
    "131.0.72.0/22",
    "2400:cb00::/32",
    "2606:4700::/32",
    "2803:f800::/32",
    "2405:b500::/32",
    "2405:8100::/32",
    "2a06:98c0::/29",
    "2c0f:f248::/32",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cidr {
    network: IpAddr,
    prefix: u8,
}

impl Cidr {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (addr, prefix) = match text.split_once('/') {
            Some((addr, prefix)) => (addr, prefix.parse::<u8>().ok()?),
            None => {
                let addr: IpAddr = text.parse().ok()?;
                let full = if addr.is_ipv4() { 32 } else { 128 };
                return Some(Self {
                    network: addr,
                    prefix: full,
                });
            }
        };
        let network: IpAddr = addr.parse().ok()?;
        let max = if network.is_ipv4() { 32 } else { 128 };
        (prefix <= max).then_some(Self { network, prefix })
    }

    fn contains(&self, ip: IpAddr) -> bool {
        match (self.network, canonical(ip)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// IPv4-mapped IPv6(::ffff:a.b.c.d,双栈监听时常见)还原成 IPv4 再比较。
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        other => other,
    }
}

#[derive(Clone, Debug, Default)]
pub struct SourceFilter {
    /// None = 不限制
    allowed: Option<Vec<Cidr>>,
}

impl SourceFilter {
    pub fn from_env() -> Self {
        Self::from_settings(
            std::env::var("RUSTPANEL_PANEL_TRUSTED_SOURCES")
                .ok()
                .as_deref(),
            std::env::var("RUSTPANEL_PANEL_ALLOW_CIDRS").ok().as_deref(),
        )
    }

    fn from_settings(trusted: Option<&str>, extra: Option<&str>) -> Self {
        let trusted = trusted.map(str::trim).unwrap_or_default();
        if !trusted.eq_ignore_ascii_case("cloudflare") {
            if !trusted.is_empty() {
                tracing::warn!(
                    trusted,
                    "unknown RUSTPANEL_PANEL_TRUSTED_SOURCES, not filtering"
                );
            }
            return Self::default();
        }
        let mut allowed: Vec<Cidr> = CLOUDFLARE_RANGES
            .iter()
            .filter_map(|range| Cidr::parse(range))
            .collect();
        for item in extra.unwrap_or_default().split(',') {
            if item.trim().is_empty() {
                continue;
            }
            match Cidr::parse(item) {
                Some(cidr) => allowed.push(cidr),
                None => tracing::warn!(item, "ignoring invalid RUSTPANEL_PANEL_ALLOW_CIDRS entry"),
            }
        }
        Self {
            allowed: Some(allowed),
        }
    }

    pub fn is_active(&self) -> bool {
        self.allowed.is_some()
    }

    pub fn allows(&self, ip: IpAddr) -> bool {
        let Some(allowed) = &self.allowed else {
            return true;
        };
        let ip = canonical(ip);
        ip.is_loopback() || allowed.iter().any(|cidr| cidr.contains(ip))
    }
}

/// axum::serve 的 make-service:按连接来源决定放行还是一律 403。
#[derive(Clone)]
pub struct GatedMakeService<S> {
    inner: S,
    filter: Arc<SourceFilter>,
}

impl<S> GatedMakeService<S> {
    pub fn new(inner: S, filter: SourceFilter) -> Self {
        Self {
            inner,
            filter: Arc::new(filter),
        }
    }
}

impl<S: Clone> Service<IncomingStream<'_, tokio::net::TcpListener>> for GatedMakeService<S> {
    type Response = Gate<S>;
    type Error = Infallible;
    type Future = Ready<Result<Gate<S>, Infallible>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, incoming: IncomingStream<'_, tokio::net::TcpListener>) -> Self::Future {
        let allowed = self.filter.allows(incoming.remote_addr().ip());
        ready(Ok(Gate {
            inner: self.inner.clone(),
            allowed,
        }))
    }
}

#[derive(Clone)]
pub struct Gate<S> {
    inner: S,
    allowed: bool,
}

type GateFuture =
    Pin<Box<dyn std::future::Future<Output = Result<HttpResponse<Body>, Infallible>> + Send>>;

impl<S> Service<Request> for Gate<S>
where
    S: Service<Request, Response = HttpResponse<Body>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = HttpResponse<Body>;
    type Error = Infallible;
    type Future = GateFuture;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        if !self.allowed {
            return Box::pin(ready(Ok(forbidden())));
        }
        let inner = self.inner.clone();
        Box::pin(inner.oneshot(request))
    }
}

fn forbidden() -> HttpResponse<Body> {
    let mut response = HttpResponse::new(Body::from(
        "403: this panel only accepts connections through its domain (Cloudflare)\n",
    ));
    *response.status_mut() = StatusCode::FORBIDDEN;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("ip")
    }

    #[test]
    fn disabled_by_default_allows_everyone() {
        let filter = SourceFilter::from_settings(None, None);
        assert!(!filter.is_active());
        assert!(filter.allows(ip("8.8.8.8")));
    }

    #[test]
    fn cloudflare_mode_allows_cf_loopback_and_extra_only() {
        let filter =
            SourceFilter::from_settings(Some("cloudflare"), Some("203.0.113.7, 10.0.0.0/8,bogus"));
        assert!(filter.is_active());
        // 线上实测过的 Cloudflare 回源地址
        assert!(filter.allows(ip("104.23.160.167")));
        assert!(filter.allows(ip("2606:4700:10::6814:1")));
        assert!(filter.allows(ip("::ffff:104.23.160.167")), "ipv4-mapped");
        assert!(filter.allows(ip("127.0.0.1")));
        assert!(filter.allows(ip("::1")));
        assert!(filter.allows(ip("203.0.113.7")));
        assert!(filter.allows(ip("10.1.2.3")));
        assert!(!filter.allows(ip("8.8.8.8")));
        assert!(
            !filter.allows(ip("104.32.0.1")),
            "outside every Cloudflare range"
        );
        assert!(!filter.allows(ip("2001:db8::1")));
    }

    #[test]
    fn cidr_parsing_rejects_bad_prefixes() {
        assert!(Cidr::parse("10.0.0.0/33").is_none());
        assert!(Cidr::parse("::/129").is_none());
        assert!(Cidr::parse("nope").is_none());
        assert_eq!(Cidr::parse("1.2.3.4").map(|cidr| cidr.prefix), Some(32));
        assert!(Cidr::parse("0.0.0.0/0").is_some_and(|cidr| cidr.contains(ip("9.9.9.9"))));
    }
}
