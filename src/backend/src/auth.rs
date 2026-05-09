use std::{
    env,
    fmt::{Display, Formatter},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use tonic::{metadata::MetadataValue, service::Interceptor, Request, Status};

const DEFAULT_ISSUER: &str = "rustpanel";
const DEFAULT_TOKEN_TTL_SECONDS: u64 = 86_400;
const MIN_SECRET_BYTES: usize = 32;
const DEV_ONLY_SECRET: &str = "rustpanel-dev-only-secret-change-before-production";
const AUTHORIZATION_HEADER: &str = "authorization";
const BEARER_PREFIX: &str = "Bearer ";

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

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    EmptySubject,
    ExpiredOrInvalidToken,
    InvalidTokenTtl,
    JwtSecretTooShort { min_bytes: usize },
    MissingProductionSecret,
    TimeOverflow,
}

impl Display for AuthError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySubject => write!(formatter, "subject must not be empty"),
            Self::ExpiredOrInvalidToken => write!(formatter, "token is expired or invalid"),
            Self::InvalidTokenTtl => write!(formatter, "JWT TTL must be greater than zero"),
            Self::JwtSecretTooShort { min_bytes } => {
                write!(formatter, "JWT secret must be at least {min_bytes} bytes")
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
}
