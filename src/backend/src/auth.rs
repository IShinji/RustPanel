use std::{
    env,
    fmt::{Display, Formatter},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use tonic::{
    metadata::MetadataValue, service::Interceptor, Request, Response as GrpcResponse, Status,
};

use crate::{
    audit, error_response, ok_response,
    proto::rustpanel::v1::{
        auth_service_server::AuthService, LoginRequest, LoginResponse, LogoutRequest,
        LogoutResponse, TokenRefreshRequest, TokenRefreshResponse,
    },
    security::SecurityConfig,
};

const DEFAULT_ISSUER: &str = "rustpanel";
const DEFAULT_TOKEN_TTL_SECONDS: u64 = 86_400;
const MIN_SECRET_BYTES: usize = 32;
const DEV_ONLY_SECRET: &str = "rustpanel-dev-only-secret-change-before-production";
const AUTHORIZATION_HEADER: &str = "authorization";
const BEARER_PREFIX: &str = "Bearer ";
const DEFAULT_ADMIN_USERNAME: &str = "admin";
const DEFAULT_ADMIN_PASSWORD: &str = "rustpanel";
const TOTP_STEP_SECONDS: u64 = 30;
const TOTP_DIGITS: u32 = 6;
const REFRESH_SUBJECT_PREFIX: &str = "refresh:";
// 面板登录失败激增告警:窗口内累计达阈值且过冷却才推一次(聚合,避免每次失败都发)。
const LOGIN_FAIL_WINDOW_SECONDS: u64 = 600;
const LOGIN_FAIL_THRESHOLD: usize = 5;
const LOGIN_ALERT_COOLDOWN_SECONDS: u64 = 3600;

type HmacSha1 = Hmac<Sha1>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JwtClaims {
    pub sub: String,
    pub iss: String,
    pub iat: u64,
    pub exp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuedToken {
    pub token: String,
    pub expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedSubject {
    pub subject: String,
}

#[derive(Clone, Debug)]
pub struct JwtAuthority {
    secret: Arc<[u8]>,
    issuer: String,
    ttl: Duration,
}

#[derive(Clone, Debug)]
pub struct AuthServiceImpl {
    authority: JwtAuthority,
    credentials: PanelCredentials,
    security: SecurityConfig,
    totp_secret: Option<Vec<u8>>,
    // 登录失败内存滑窗 + 上次告警时间;用于"登录失败激增"聚合通知(默认关)。
    recent_failures: Arc<tokio::sync::Mutex<Vec<u64>>>,
    last_login_alert: Arc<tokio::sync::Mutex<u64>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PanelCredentials {
    username: String,
    password: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    EmptySubject,
    ExpiredOrInvalidToken,
    InvalidAdminPassword,
    InvalidTotpSecret,
    InvalidTokenTtl,
    JwtSecretTooShort { min_bytes: usize },
    MissingProductionAdminPassword,
    MissingProductionSecret,
    TimeOverflow,
}

impl Display for AuthError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySubject => write!(formatter, "subject must not be empty"),
            Self::ExpiredOrInvalidToken => write!(formatter, "token is expired or invalid"),
            Self::InvalidAdminPassword => write!(formatter, "admin password must not be empty"),
            Self::InvalidTotpSecret => write!(formatter, "TOTP secret must be valid base32"),
            Self::InvalidTokenTtl => write!(formatter, "JWT TTL must be greater than zero"),
            Self::JwtSecretTooShort { min_bytes } => {
                write!(formatter, "JWT secret must be at least {min_bytes} bytes")
            }
            Self::MissingProductionAdminPassword => {
                write!(
                    formatter,
                    "RUSTPANEL_ADMIN_PASSWORD is required in production"
                )
            }
            Self::MissingProductionSecret => {
                write!(formatter, "RUSTPANEL_JWT_SECRET is required in production")
            }
            Self::TimeOverflow => write!(formatter, "JWT timestamp overflow"),
        }
    }
}

impl std::error::Error for AuthError {}

impl JwtAuthority {
    pub fn new(
        secret: impl Into<Vec<u8>>,
        issuer: impl Into<String>,
        ttl: Duration,
    ) -> Result<Self, AuthError> {
        let secret = secret.into();
        if secret.len() < MIN_SECRET_BYTES {
            return Err(AuthError::JwtSecretTooShort {
                min_bytes: MIN_SECRET_BYTES,
            });
        }
        if ttl.is_zero() {
            return Err(AuthError::InvalidTokenTtl);
        }

        Ok(Self {
            secret: Arc::from(secret),
            issuer: issuer.into(),
            ttl,
        })
    }

    pub fn from_env() -> Result<Self, AuthError> {
        let secret = match env::var("RUSTPANEL_JWT_SECRET") {
            Ok(secret) => secret,
            Err(_) if is_production_env() => return Err(AuthError::MissingProductionSecret),
            Err(_) => DEV_ONLY_SECRET.to_owned(),
        };
        let issuer = env::var("RUSTPANEL_JWT_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_owned());
        let ttl = env::var("RUSTPANEL_JWT_TTL_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_TOKEN_TTL_SECONDS);

        Self::new(secret.into_bytes(), issuer, Duration::from_secs(ttl))
    }

    pub fn issue(&self, subject: impl Into<String>) -> Result<IssuedToken, AuthError> {
        self.issue_at(subject, unix_now()?)
    }

    pub fn validate(&self, token: &str) -> Result<JwtClaims, AuthError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[self.issuer.as_str()]);

        let claims =
            decode::<JwtClaims>(token, &DecodingKey::from_secret(&self.secret), &validation)
                .map_err(|_| AuthError::ExpiredOrInvalidToken)?
                .claims;

        if claims.sub.trim().is_empty() {
            return Err(AuthError::EmptySubject);
        }

        Ok(claims)
    }

    fn issue_at(
        &self,
        subject: impl Into<String>,
        issued_at: u64,
    ) -> Result<IssuedToken, AuthError> {
        let subject = subject.into();
        if subject.trim().is_empty() {
            return Err(AuthError::EmptySubject);
        }

        let expires_at = issued_at
            .checked_add(self.ttl.as_secs())
            .ok_or(AuthError::TimeOverflow)?;
        let claims = JwtClaims {
            sub: subject,
            iss: self.issuer.clone(),
            iat: issued_at,
            exp: expires_at,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&self.secret),
        )
        .map_err(|_| AuthError::ExpiredOrInvalidToken)?;

        Ok(IssuedToken { token, expires_at })
    }
}

impl AuthServiceImpl {
    pub fn from_env(authority: JwtAuthority) -> Result<Self, AuthError> {
        Ok(Self {
            authority,
            credentials: PanelCredentials::from_env()?,
            security: SecurityConfig::from_env(),
            totp_secret: totp_secret_from_env()?,
            recent_failures: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            last_login_alert: Arc::new(tokio::sync::Mutex::new(0)),
        })
    }

    /// 记一次面板登录失败;窗口内累计达阈值且过了冷却,推送"登录失败激增"告警。
    /// notify_event 自身按 LOGIN_FAILED 规则开关决定是否真的发(默认关)。
    async fn note_login_failure(&self) {
        let now = unix_now().unwrap_or(0);
        let count = {
            let mut fails = self.recent_failures.lock().await;
            fails.push(now);
            fails.retain(|ts| now.saturating_sub(*ts) <= LOGIN_FAIL_WINDOW_SECONDS);
            fails.len()
        };
        if count < LOGIN_FAIL_THRESHOLD {
            return;
        }
        {
            let mut last = self.last_login_alert.lock().await;
            if *last != 0 && now.saturating_sub(*last) < LOGIN_ALERT_COOLDOWN_SECONDS {
                return;
            }
            *last = now;
        }
        tokio::spawn(async move {
            crate::notification::notify_event(
                crate::proto::rustpanel::v1::NotificationEventKind::LoginFailed,
                "面板登录失败激增",
                &format!(
                    "最近 {LOGIN_FAIL_WINDOW_SECONDS} 秒内出现 {count} 次面板登录失败,可能正在被爆破。"
                ),
            )
            .await;
        });
    }
}

#[tonic::async_trait]
impl AuthService for AuthServiceImpl {
    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<GrpcResponse<LoginResponse>, Status> {
        let request = request.into_inner();
        if !self
            .credentials
            .matches(&request.username, &request.password)
        {
            let _ = audit::append_audit_event(
                "auth",
                "login_failed",
                format!("login failed for {}", request.username),
                "grpc",
            )
            .await;
            self.note_login_failure().await;
            return Err(Status::unauthenticated("invalid username or password"));
        }

        let requires_two_factor =
            self.security.two_factor_required().await || self.totp_secret.is_some();
        if requires_two_factor {
            let secret = self
                .totp_secret
                .as_deref()
                .ok_or_else(|| Status::failed_precondition("TOTP secret is not configured"))?;
            if request.totp_code.trim().is_empty()
                || !verify_totp(
                    secret,
                    request.totp_code.trim(),
                    unix_now().map_err(auth_status)?,
                )
            {
                let _ = audit::append_audit_event(
                    "auth",
                    "login_failed_2fa",
                    format!("two factor required for {}", request.username),
                    "grpc",
                )
                .await;
                self.note_login_failure().await;
                return Ok(GrpcResponse::new(LoginResponse {
                    status: Some(error_response(401, "two factor code required")),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    expires_at: 0,
                    requires_two_factor: true,
                }));
            }
        }

        let issued = self
            .authority
            .issue(&request.username)
            .map_err(auth_status)?;
        let refresh = self
            .authority
            .issue(format!("{REFRESH_SUBJECT_PREFIX}{}", request.username))
            .map_err(auth_status)?;
        let _ = audit::append_audit_event(
            "auth",
            "login_success",
            format!("login succeeded for {}", request.username),
            "grpc",
        )
        .await;

        Ok(GrpcResponse::new(LoginResponse {
            status: Some(ok_response("login ok")),
            access_token: issued.token,
            refresh_token: refresh.token,
            expires_at: issued.expires_at,
            requires_two_factor: false,
        }))
    }

    async fn logout(
        &self,
        _request: Request<LogoutRequest>,
    ) -> Result<GrpcResponse<LogoutResponse>, Status> {
        let _ = audit::append_audit_event("auth", "logout", "panel logout", "grpc").await;
        Ok(GrpcResponse::new(LogoutResponse {
            status: Some(ok_response("logout ok")),
        }))
    }

    async fn token_refresh(
        &self,
        request: Request<TokenRefreshRequest>,
    ) -> Result<GrpcResponse<TokenRefreshResponse>, Status> {
        let claims = self
            .authority
            .validate(&request.into_inner().refresh_token)
            .map_err(auth_status)?;
        let subject = claims
            .sub
            .strip_prefix(REFRESH_SUBJECT_PREFIX)
            .ok_or_else(|| Status::unauthenticated("invalid refresh token"))?;
        let issued = self.authority.issue(subject).map_err(auth_status)?;

        Ok(GrpcResponse::new(TokenRefreshResponse {
            status: Some(ok_response("token refreshed")),
            access_token: issued.token,
            expires_at: issued.expires_at,
        }))
    }
}

impl PanelCredentials {
    fn from_env() -> Result<Self, AuthError> {
        let username = env::var("RUSTPANEL_ADMIN_USERNAME")
            .unwrap_or_else(|_| DEFAULT_ADMIN_USERNAME.to_owned());
        let password = match env::var("RUSTPANEL_ADMIN_PASSWORD") {
            Ok(password) => password,
            Err(_) if is_production_env() => return Err(AuthError::MissingProductionAdminPassword),
            Err(_) => DEFAULT_ADMIN_PASSWORD.to_owned(),
        };
        if password.trim().is_empty() {
            return Err(AuthError::InvalidAdminPassword);
        }
        Ok(Self { username, password })
    }

    fn matches(&self, username: &str, password: &str) -> bool {
        constant_time_eq(self.username.as_bytes(), username.as_bytes())
            & constant_time_eq(self.password.as_bytes(), password.as_bytes())
    }
}

#[derive(Clone, Debug)]
pub struct AuthInterceptor {
    authority: Arc<JwtAuthority>,
}

impl AuthInterceptor {
    pub fn new(authority: JwtAuthority) -> Self {
        Self {
            authority: Arc::new(authority),
        }
    }

    #[allow(clippy::result_large_err)]
    fn authenticate(&self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let token = bearer_token(request.metadata().get(AUTHORIZATION_HEADER))?;
        let claims = self
            .authority
            .validate(token)
            .map_err(|_| Status::unauthenticated("invalid bearer token"))?;

        request.extensions_mut().insert(AuthenticatedSubject {
            subject: claims.sub,
        });

        Ok(request)
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        self.authenticate(request)
    }
}

#[allow(clippy::result_large_err)]
fn bearer_token(value: Option<&MetadataValue<tonic::metadata::Ascii>>) -> Result<&str, Status> {
    let value = value.ok_or_else(|| Status::unauthenticated("missing authorization header"))?;
    let value = value
        .to_str()
        .map_err(|_| Status::unauthenticated("authorization header must be ASCII"))?;
    let token = value
        .strip_prefix(BEARER_PREFIX)
        .ok_or_else(|| Status::unauthenticated("authorization header must use Bearer scheme"))?;

    if token.trim().is_empty() || token.chars().any(char::is_whitespace) {
        return Err(Status::unauthenticated(
            "bearer token is empty or malformed",
        ));
    }

    Ok(token)
}

fn totp_secret_from_env() -> Result<Option<Vec<u8>>, AuthError> {
    env::var("RUSTPANEL_TOTP_SECRET")
        .or_else(|_| env::var("RUSTPANEL_2FA_SECRET"))
        .ok()
        .filter(|secret| !secret.trim().is_empty())
        .map(|secret| decode_totp_secret(&secret))
        .transpose()
}

fn decode_totp_secret(secret: &str) -> Result<Vec<u8>, AuthError> {
    let normalized = secret
        .chars()
        .filter(|char| !char.is_whitespace() && *char != '=')
        .flat_map(char::to_uppercase)
        .collect::<String>();
    BASE32_NOPAD
        .decode(normalized.as_bytes())
        .map_err(|_| AuthError::InvalidTotpSecret)
}

fn verify_totp(secret: &[u8], code: &str, timestamp_seconds: u64) -> bool {
    if code.len() != TOTP_DIGITS as usize || !code.chars().all(|char| char.is_ascii_digit()) {
        return false;
    }
    let counter = timestamp_seconds / TOTP_STEP_SECONDS;
    (counter.saturating_sub(1)..=counter.saturating_add(1)).any(|candidate| {
        totp_code(secret, candidate)
            .map(|expected| constant_time_eq(expected.as_bytes(), code.as_bytes()))
            .unwrap_or(false)
    })
}

fn totp_code(secret: &[u8], counter: u64) -> Result<String, AuthError> {
    let mut mac = HmacSha1::new_from_slice(secret).map_err(|_| AuthError::InvalidTotpSecret)?;
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let binary = ((u32::from(digest[offset]) & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    let modulo = 10_u32.pow(TOTP_DIGITS);
    Ok(format!(
        "{:0width$}",
        binary % modulo,
        width = TOTP_DIGITS as usize
    ))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for index in 0..max_len {
        let left_byte = left.get(index).copied().unwrap_or_default();
        let right_byte = right.get(index).copied().unwrap_or_default();
        diff |= usize::from(left_byte ^ right_byte);
    }
    diff == 0
}

fn auth_status(error: AuthError) -> Status {
    Status::unauthenticated(error.to_string())
}

fn unix_now() -> Result<u64, AuthError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AuthError::TimeOverflow)
        .map(|duration| duration.as_secs())
}

fn is_production_env() -> bool {
    env::var("RUSTPANEL_ENV")
        .or_else(|_| env::var("APP_ENV"))
        .is_ok_and(|value| value.eq_ignore_ascii_case("production"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::metadata::MetadataValue;

    fn authority() -> JwtAuthority {
        JwtAuthority::new(
            "0123456789abcdef0123456789abcdef".as_bytes().to_vec(),
            "rustpanel-test",
            Duration::from_secs(60),
        )
        .expect("authority")
    }

    #[test]
    fn jwt_round_trip_preserves_subject() {
        let authority = authority();
        let issued_at = unix_now().expect("now");
        let issued = authority.issue_at("admin", issued_at).expect("token");
        let claims = authority.validate(&issued.token).expect("claims");

        assert_eq!(claims.sub, "admin");
        assert_eq!(claims.iss, "rustpanel-test");
        assert_eq!(issued.expires_at, issued_at + 60);
    }

    #[test]
    fn jwt_rejects_short_secret() {
        let result = JwtAuthority::new(
            "too-short".as_bytes().to_vec(),
            "rustpanel",
            Duration::from_secs(60),
        );

        assert_eq!(
            result.expect_err("short secret"),
            AuthError::JwtSecretTooShort {
                min_bytes: MIN_SECRET_BYTES
            }
        );
    }

    #[test]
    fn interceptor_accepts_valid_bearer_token() {
        let authority = authority();
        let issued = authority.issue("admin").expect("token");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            AUTHORIZATION_HEADER,
            MetadataValue::try_from(format!("{BEARER_PREFIX}{}", issued.token)).expect("metadata"),
        );

        let request = AuthInterceptor::new(authority)
            .authenticate(request)
            .expect("authenticated");
        let subject = request
            .extensions()
            .get::<AuthenticatedSubject>()
            .expect("subject");

        assert_eq!(subject.subject, "admin");
    }

    #[test]
    fn interceptor_rejects_missing_or_malformed_token() {
        let interceptor = AuthInterceptor::new(authority());

        assert_eq!(
            interceptor
                .authenticate(Request::new(()))
                .expect_err("missing")
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut malformed = Request::new(());
        malformed.metadata_mut().insert(
            AUTHORIZATION_HEADER,
            MetadataValue::try_from("Basic abc").expect("metadata"),
        );

        assert_eq!(
            interceptor
                .authenticate(malformed)
                .expect_err("malformed")
                .code(),
            tonic::Code::Unauthenticated
        );
    }

    #[test]
    fn totp_matches_rfc6238_vector_truncated_to_six_digits() {
        let secret = decode_totp_secret("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").expect("secret");
        let code = totp_code(&secret, 59 / TOTP_STEP_SECONDS).expect("code");

        assert_eq!(code, "287082");
        assert!(verify_totp(&secret, "287082", 59));
        assert!(!verify_totp(&secret, "000000", 59));
    }

    #[test]
    fn constant_time_eq_requires_same_bytes() {
        assert!(constant_time_eq(b"rustpanel", b"rustpanel"));
        assert!(!constant_time_eq(b"rustpanel", b"rustpanel2"));
        assert!(!constant_time_eq(b"rustpanel", b"rustpanic"));
    }
}
