use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

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
        // 角色或密码变了 → 旧 token 立刻作废(降权/改密必须马上生效)。
        let credentials_changed = existing
            .as_ref()
            .map(|old| old.role != stored.role || old.hash != stored.hash)
            .unwrap_or(false);
        if credentials_changed {
            revoke_tokens_in(&mut state, &request.username, now);
        }
        state.users.retain(|u| u.username != request.username);
        state.users.push(stored.clone());
        self.store.save(&state).await?;
        refresh_revocation_cache(&self.store, &state).await;

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
        // 用户没了,但他手上的 token 还没到期 —— 一并吊销。
        revoke_tokens_in(&mut state, &username, current_timestamp());
        self.store.save(&state).await?;
        refresh_revocation_cache(&self.store, &state).await;
        Ok(GrpcResponse::new(DeleteUserResponse {
            status: Some(ok_response("user deleted")),
        }))
    }
}

/// 记一次吊销水位(同名只保留最新的一条,表不会随操作次数增长)。
fn revoke_tokens_in(state: &mut StoredState, username: &str, now: u64) {
    state.revocations.retain(|entry| entry.username != username);
    state.revocations.push(StoredRevocation {
        username: username.to_owned(),
        revoked_at_seconds: now,
    });
}

type RevocationCache = tokio::sync::RwLock<Option<(PathBuf, HashMap<String, u64>)>>;

/// 吊销表的进程内缓存。带上来源路径:测试会切 `RUSTPANEL_USER_ROOT`,路径不同就重载,
/// 免得跨测试互相串味。
fn revocation_cache() -> &'static RevocationCache {
    static CACHE: OnceLock<RevocationCache> = OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::RwLock::new(None))
}

fn revocation_map(state: &StoredState) -> HashMap<String, u64> {
    state
        .revocations
        .iter()
        .map(|entry| (entry.username.clone(), entry.revoked_at_seconds))
        .collect()
}

async fn refresh_revocation_cache(store: &UserStore, state: &StoredState) {
    *revocation_cache().write().await = Some((store.state_path(), revocation_map(state)));
}

/// token 是否已被吊销:`iat <= revoked_at` 即作废。
///
/// 同一秒内「改角色 + 重新登录」会连新 token 一起判掉(JWT 的 `iat` 只有秒精度),
/// 用户重登一次即可 —— 宁可多拒一秒,不能漏放一个降权前的 token。
pub(crate) async fn token_revoked(subject: &str, issued_at: u64) -> bool {
    let username = subject
        .strip_prefix(crate::auth::REFRESH_SUBJECT_PREFIX)
        .unwrap_or(subject);
    let store = UserStore::from_env();
    let path = store.state_path();

    if let Some((cached_path, map)) = revocation_cache().read().await.as_ref() {
        if *cached_path == path {
            return map
                .get(username)
                .is_some_and(|revoked_at| issued_at <= *revoked_at);
        }
    }

    // 冷启动(或换了 state 路径):读一次盘并填缓存,之后都走内存。
    let state = store.load().await.unwrap_or_default();
    let map = revocation_map(&state);
    let revoked = map
        .get(username)
        .is_some_and(|revoked_at| issued_at <= *revoked_at);
    *revocation_cache().write().await = Some((path, map));
    revoked
}

/// 查当前存的角色;用户已不存在返回 None(refresh 时用来拒掉被删账号)。
pub(crate) async fn current_role(username: &str) -> Option<String> {
    let state = UserStore::from_env().load().await.ok()?;
    state
        .users
        .iter()
        .find(|user| user.username == username)
        .map(|user| role_to_str(user.role).to_owned())
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
        // 存的是 PBKDF2 口令哈希 + salt,按机密落盘(0600)。
        crate::statefile::write_secret_atomic(&self.state_path(), content)
            .await
            .map_err(io_status)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct StoredState {
    #[serde(default)]
    users: Vec<StoredUser>,
    /// 令牌吊销登记表。serde default 让旧状态文件(无此字段)照常读。
    #[serde(default)]
    revocations: Vec<StoredRevocation>,
}

/// 「该用户在 `revoked_at_seconds`(含)之前签发的 JWT 一律作废」。
///
/// JWT 是自包含的,角色写死在 token 里:改角色、改密码、删用户之后,旧 token 在
/// TTL(默认 24h)内依然全权有效 —— 降权和封禁等于不生效。这里用一张极小的
/// 「吊销水位表」补上:变更时记 `now`,校验时比 `iat`。表随用户数增长(删号也只
/// 留一行),常驻内存缓存,不给每个请求加一次磁盘读。
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredRevocation {
    username: String,
    revoked_at_seconds: u64,
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

    #[test]
    fn revoke_keeps_one_row_per_user_and_takes_latest() {
        let mut state = StoredState::default();
        revoke_tokens_in(&mut state, "ops", 100);
        revoke_tokens_in(&mut state, "ops", 200);
        revoke_tokens_in(&mut state, "other", 150);

        assert_eq!(state.revocations.len(), 2);
        let map = revocation_map(&state);
        assert_eq!(map.get("ops"), Some(&200));
        assert_eq!(map.get("other"), Some(&150));
    }

    #[test]
    fn old_state_file_without_revocations_still_loads() {
        let state: StoredState =
            serde_json::from_str(r#"{"users":[]}"#).expect("legacy state parses");
        assert!(state.revocations.is_empty());
    }

    // RUSTPANEL_USER_ROOT 是进程级 env var,串行化后再改,避免并发测试互相踩。
    fn env_guard() -> &'static tokio::sync::Mutex<()> {
        static GUARD: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        GUARD.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[tokio::test]
    async fn revoked_user_tokens_are_rejected_until_reissued() {
        let _guard = env_guard().lock().await;
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("RUSTPANEL_USER_ROOT", dir.path());

        let store = UserStore::from_env();
        let mut state = StoredState::default();
        revoke_tokens_in(&mut state, "ops", 1_000);
        store.save(&state).await.expect("save");
        refresh_revocation_cache(&store, &state).await;

        // 吊销时刻(含)之前签发的一律作废,之后签发的放行。
        assert!(token_revoked("ops", 999).await);
        assert!(token_revoked("ops", 1_000).await);
        assert!(!token_revoked("ops", 1_001).await);
        // refresh token 的 subject 带前缀,同样按用户名判定。
        assert!(token_revoked("refresh:ops", 999).await);
        // 没登记过的用户不受影响。
        assert!(!token_revoked("admin", 1).await);

        std::env::remove_var("RUSTPANEL_USER_ROOT");
    }

    #[tokio::test]
    async fn deleting_user_records_revocation_watermark() {
        let _guard = env_guard().lock().await;
        let dir = tempfile::tempdir().expect("tempdir");
        std::env::set_var("RUSTPANEL_USER_ROOT", dir.path());

        let service = UserServiceImpl::new();
        service
            .upsert_user(Request::new(UpsertUserRequest {
                username: "ops".into(),
                password: "hunter2".into(),
                role: UserRole::Operator as i32,
            }))
            .await
            .expect("create");
        assert_eq!(
            verify_user("ops", "hunter2").await.as_deref(),
            Some("operator")
        );

        service
            .delete_user(Request::new(DeleteUserRequest {
                username: "ops".into(),
            }))
            .await
            .expect("delete");

        let state = service.store.load().await.expect("load");
        assert!(state.users.is_empty());
        assert_eq!(state.revocations.len(), 1);
        // 删号之前签发的 token 立刻失效。
        assert!(token_revoked("ops", 0).await);

        std::env::remove_var("RUSTPANEL_USER_ROOT");
    }
}
