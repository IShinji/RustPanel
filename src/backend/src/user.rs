use std::{env, path::PathBuf, sync::Arc};

use data_encoding::HEXLOWER;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        user_service_server::UserService, DeleteUserRequest, DeleteUserResponse, ListUsersRequest,
        ListUsersResponse, UpsertUserRequest, UpsertUserResponse, User, UserRole,
    },
};

const DEFAULT_USER_ROOT: &str = "/tmp/rustpanel/users";
const PBKDF2_ITERATIONS: u32 = 100_000;

#[derive(Clone)]
pub struct UserServiceImpl {
    store: UserStore,
}

impl UserServiceImpl {
    pub fn new() -> Self {
        Self {
            store: UserStore::from_env(),
        }
    }
}

impl Default for UserServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl UserService for UserServiceImpl {
    async fn list_users(
        &self,
        _request: Request<ListUsersRequest>,
    ) -> Result<GrpcResponse<ListUsersResponse>, Status> {
        let state = self.store.load().await?;
        Ok(GrpcResponse::new(ListUsersResponse {
            status: Some(ok_response("ok")),
            users: state
                .users
                .into_iter()
                .map(StoredUser::into_proto)
                .collect(),
        }))
    }

    async fn upsert_user(
        &self,
        request: Request<UpsertUserRequest>,
    ) -> Result<GrpcResponse<UpsertUserResponse>, Status> {
        let request = request.into_inner();
        validate_username(&request.username)?;
        let role = UserRole::try_from(request.role).unwrap_or(UserRole::Unspecified);
        if role == UserRole::Unspecified {
            return Err(Status::invalid_argument("user role is required"));
        }

        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        let now = current_timestamp();
        let existing = state
            .users
            .iter()
            .find(|u| u.username == request.username)
            .cloned();

        let (salt, hash, iterations, created_at) = if request.password.trim().is_empty() {
            // 编辑且留空 → 保留原密码;新增则必须给密码。
            match &existing {
                Some(old) => (
                    old.salt.clone(),
                    old.hash.clone(),
                    old.iterations,
                    old.created_at_seconds,
                ),
                None => {
                    return Err(Status::invalid_argument(
                        "password is required for new user",
                    ))
                }
            }
        } else {
            let salt = random_salt();
            let hash = pbkdf2_hex(request.password.as_bytes(), &salt, PBKDF2_ITERATIONS);
            let created_at = existing
                .as_ref()
                .map(|o| o.created_at_seconds)
                .unwrap_or(now);
            (HEXLOWER.encode(&salt), hash, PBKDF2_ITERATIONS, created_at)
        };

        let stored = StoredUser {
            username: request.username.clone(),
            salt,
            hash,
            iterations,
            role: role as i32,
            created_at_seconds: created_at,
        };
        state.users.retain(|u| u.username != request.username);
        state.users.push(stored.clone());
        self.store.save(&state).await?;

        Ok(GrpcResponse::new(UpsertUserResponse {
            status: Some(ok_response("user saved")),
            user: Some(stored.into_proto()),
        }))
    }

    async fn delete_user(
        &self,
        request: Request<DeleteUserRequest>,
    ) -> Result<GrpcResponse<DeleteUserResponse>, Status> {
        let username = request.into_inner().username;
        let _guard = self.store.write_lock.lock().await;
        let mut state = self.store.load().await?;
        let before = state.users.len();
        state.users.retain(|u| u.username != username);
        if state.users.len() == before {
            return Err(Status::not_found("user not found"));
        }
        self.store.save(&state).await?;
        Ok(GrpcResponse::new(DeleteUserResponse {
            status: Some(ok_response("user deleted")),
        }))
    }
}

/// 校验用户名/密码,成功返回角色字符串("admin"/"operator"/"readonly")供写入 JWT。
pub(crate) async fn verify_user(username: &str, password: &str) -> Option<String> {
    let store = UserStore::from_env();
    let state = store.load().await.ok()?;
    let user = state.users.iter().find(|u| u.username == username)?;
    let salt = HEXLOWER.decode(user.salt.as_bytes()).ok()?;
    let computed = pbkdf2_hex(password.as_bytes(), &salt, user.iterations);
    if constant_time_eq(computed.as_bytes(), user.hash.as_bytes()) {
        Some(role_to_str(user.role).to_owned())
    } else {
        None
    }
}

/// UserRole(i32) → JWT 里用的短角色串。Unspecified 兜底为 admin(不应出现)。
pub(crate) fn role_to_str(role: i32) -> &'static str {
    match UserRole::try_from(role).unwrap_or(UserRole::Unspecified) {
        UserRole::Operator => "operator",
        UserRole::Readonly => "readonly",
        UserRole::Admin | UserRole::Unspecified => "admin",
    }
}

fn validate_username(username: &str) -> Result<(), Status> {
    let valid = !username.is_empty()
        && username.len() <= 64
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if valid {
        Ok(())
    } else {
        Err(Status::invalid_argument(
            "username must be 1-64 chars of [a-zA-Z0-9_.-]",
        ))
    }
}

fn random_salt() -> Vec<u8> {
    uuid::Uuid::new_v4().as_bytes().to_vec()
}

type HmacSha256 = Hmac<Sha256>;

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    let bytes = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}

/// 单块 PBKDF2-HMAC-SHA256(输出 32 字节,正好一块)。
fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut salted = salt.to_vec();
    salted.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac_sha256(password, &salted);
    let mut result = u;
    for _ in 1..iterations.max(1) {
        u = hmac_sha256(password, &u);
        for (acc, byte) in result.iter_mut().zip(u.iter()) {
            *acc ^= byte;
        }
    }
    result
}

fn pbkdf2_hex(password: &[u8], salt: &[u8], iterations: u32) -> String {
    HEXLOWER.encode(&pbkdf2_sha256(password, salt, iterations))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        let l = left.get(index).copied().unwrap_or_default();
        let r = right.get(index).copied().unwrap_or_default();
        diff |= usize::from(l ^ r);
    }
    diff == 0
}

#[derive(Clone, Debug)]
struct UserStore {
    root: Arc<PathBuf>,
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl UserStore {
    fn from_env() -> Self {
        let root = env::var("RUSTPANEL_USER_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_USER_ROOT));
        Self {
            root: Arc::new(root),
            write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("users.json")
    }

    async fn load(&self) -> Result<StoredState, Status> {
        match tokio::fs::read_to_string(self.state_path()).await {
            Ok(content) => serde_json::from_str(&content).map_err(io_status),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(StoredState::default())
            }
            Err(error) => Err(io_status(error)),
        }
    }

    async fn save(&self, state: &StoredState) -> Result<(), Status> {
        tokio::fs::create_dir_all(self.root.as_ref())
            .await
            .map_err(io_status)?;
        let content = serde_json::to_string_pretty(state).map_err(io_status)?;
        let path = self.state_path();
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, content).await.map_err(io_status)?;
        tokio::fs::rename(&tmp, &path).await.map_err(io_status)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredState {
    #[serde(default)]
    users: Vec<StoredUser>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredUser {
    username: String,
    salt: String,
    hash: String,
    iterations: u32,
    role: i32,
    created_at_seconds: u64,
}

impl StoredUser {
    fn into_proto(self) -> User {
        User {
            username: self.username,
            role: self.role,
            created_at_seconds: self.created_at_seconds,
        }
    }
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_matches_rfc_vector() {
        // RFC 7914 附录中的 PBKDF2-HMAC-SHA256 向量(P="passwd", S="salt", c=1)首块。
        assert_eq!(
            HEXLOWER.encode(&pbkdf2_sha256(b"passwd", b"salt", 1)),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"
        );
    }

    #[test]
    fn hash_roundtrips_and_rejects_wrong_password() {
        let salt = random_salt();
        let hash = pbkdf2_hex(b"hunter2", &salt, 1000);
        assert!(constant_time_eq(
            pbkdf2_hex(b"hunter2", &salt, 1000).as_bytes(),
            hash.as_bytes()
        ));
        assert!(!constant_time_eq(
            pbkdf2_hex(b"wrong", &salt, 1000).as_bytes(),
            hash.as_bytes()
        ));
    }

    #[test]
    fn role_strings_map() {
        assert_eq!(role_to_str(UserRole::Operator as i32), "operator");
        assert_eq!(role_to_str(UserRole::Readonly as i32), "readonly");
        assert_eq!(role_to_str(UserRole::Admin as i32), "admin");
        assert_eq!(role_to_str(0), "admin");
    }

    #[test]
    fn validate_username_charset() {
        assert!(validate_username("ops.user-1").is_ok());
        assert!(validate_username("").is_err());
        assert!(validate_username("bad user").is_err());
    }
}
