use std::{
    env,
    path::{Path, PathBuf},
    sync::Once,
    time::UNIX_EPOCH,
};

use sqlx::{any::AnyPoolOptions, Column, Row};
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        database_service_server::DatabaseService, BackupDatabaseRequest, BackupDatabaseResponse,
        BrowseTableRequest, BrowseTableResponse, CreateDatabaseRequest, CreateDatabaseResponse,
        CreateDatabaseUserRequest, CreateDatabaseUserResponse, CreateSqliteFileRequest,
        CreateSqliteFileResponse, DatabaseItem, DatabaseOverviewRequest, DatabaseOverviewResponse,
        ExecuteSqlRequest, ExecuteSqlResponse, GetRedisInfoRequest, GetRedisInfoResponse,
        ImportSqlRequest, ImportSqlResponse, ListDatabasesRequest, ListDatabasesResponse,
        ListSqliteFilesRequest, ListSqliteFilesResponse, ListTablesRequest, ListTablesResponse,
        RedisInfo, SqlRow, SqliteFile, VacuumSqliteRequest, VacuumSqliteResponse,
    },
};

const DEFAULT_SQLITE_ROOTS: &[&str] = &[
    "/var/lib/rustpanel/sqlite",
    "/srv/sqlite",
    "/opt/rustpanel/data/sqlite",
];

static SQLX_DRIVERS: Once = Once::new();

#[derive(Clone, Debug, Default)]
pub struct DatabaseServiceImpl;

#[tonic::async_trait]
impl DatabaseService for DatabaseServiceImpl {
    async fn list_databases(
        &self,
        request: Request<ListDatabasesRequest>,
    ) -> Result<GrpcResponse<ListDatabasesResponse>, Status> {
        let request = request.into_inner();
        let pool = connect_any(&request.dsn).await?;
        let sql = match engine_from_dsn(&request.dsn)? {
            DatabaseEngineKind::Mysql => "SHOW DATABASES",
            DatabaseEngineKind::Postgres => {
                "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname"
            }
            DatabaseEngineKind::Sqlite => "SELECT 'main'",
        };
        let rows = sqlx::query(sql).fetch_all(&pool).await.map_err(db_status)?;
        let databases = rows
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>(0).ok())
            .map(|name| DatabaseItem { name })
            .collect::<Vec<_>>();

        Ok(GrpcResponse::new(ListDatabasesResponse {
            status: Some(ok_response("ok")),
            databases,
        }))
    }

    async fn create_database(
        &self,
        request: Request<CreateDatabaseRequest>,
    ) -> Result<GrpcResponse<CreateDatabaseResponse>, Status> {
        let request = request.into_inner();
        validate_identifier(&request.name)?;
        let pool = connect_any(&request.dsn).await?;
        let sql = match engine_from_dsn(&request.dsn)? {
            DatabaseEngineKind::Mysql => {
                format!("CREATE DATABASE IF NOT EXISTS `{}`", request.name)
            }
            DatabaseEngineKind::Postgres => format!("CREATE DATABASE \"{}\"", request.name),
            DatabaseEngineKind::Sqlite => {
                return Err(Status::unimplemented(
                    "SQLite creates databases from the DSN file path",
                ))
            }
        };
        sqlx::query(&sql).execute(&pool).await.map_err(db_status)?;

        Ok(GrpcResponse::new(CreateDatabaseResponse {
            status: Some(ok_response("database created")),
        }))
    }

    async fn create_database_user(
        &self,
        request: Request<CreateDatabaseUserRequest>,
    ) -> Result<GrpcResponse<CreateDatabaseUserResponse>, Status> {
        let request = request.into_inner();
        validate_identifier(&request.username)?;
        validate_identifier(&request.database)?;
        let password = sql_string_literal(&request.password);
        let pool = connect_any(&request.dsn).await?;
        match engine_from_dsn(&request.dsn)? {
            DatabaseEngineKind::Mysql => {
                let create_user = format!(
                    "CREATE USER IF NOT EXISTS '{}'@'%' IDENTIFIED BY {}",
                    request.username, password
                );
                let grant = format!(
                    "GRANT ALL PRIVILEGES ON `{}`.* TO '{}'@'%'",
                    request.database, request.username
                );
                sqlx::query(&create_user)
                    .execute(&pool)
                    .await
                    .map_err(db_status)?;
                sqlx::query(&grant)
                    .execute(&pool)
                    .await
                    .map_err(db_status)?;
            }
            DatabaseEngineKind::Postgres => {
                let create_user = format!(
                    "DO $$ BEGIN CREATE USER \"{}\" WITH PASSWORD {}; EXCEPTION WHEN duplicate_object THEN NULL; END $$;",
                    request.username, password
                );
                let grant = format!(
                    "GRANT ALL PRIVILEGES ON DATABASE \"{}\" TO \"{}\"",
                    request.database, request.username
                );
                sqlx::query(&create_user)
                    .execute(&pool)
                    .await
                    .map_err(db_status)?;
                sqlx::query(&grant)
                    .execute(&pool)
                    .await
                    .map_err(db_status)?;
            }
            DatabaseEngineKind::Sqlite => {
                return Err(Status::unimplemented("SQLite does not support users"));
            }
        }

        Ok(GrpcResponse::new(CreateDatabaseUserResponse {
            status: Some(ok_response("database user created")),
        }))
    }

    async fn execute_sql(
        &self,
        request: Request<ExecuteSqlRequest>,
    ) -> Result<GrpcResponse<ExecuteSqlResponse>, Status> {
        let request = request.into_inner();
        let pool = connect_any(&request.dsn).await?;
        if returns_rows(&request.sql) {
            let rows = sqlx::query(&request.sql)
                .fetch_all(&pool)
                .await
                .map_err(db_status)?;
            let columns = rows
                .first()
                .map(|row| {
                    row.columns()
                        .iter()
                        .map(|column| column.name().to_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let max_rows = usize::try_from(request.max_rows.max(100)).unwrap_or(100);
            let rows = rows
                .into_iter()
                .take(max_rows)
                .map(|row| SqlRow {
                    values: (0..row.len())
                        .map(|index| value_to_string(&row, index))
                        .collect(),
                })
                .collect::<Vec<_>>();

            Ok(GrpcResponse::new(ExecuteSqlResponse {
                status: Some(ok_response("ok")),
                columns,
                rows,
                rows_affected: 0,
            }))
        } else {
            let result = sqlx::query(&request.sql)
                .execute(&pool)
                .await
                .map_err(db_status)?;
            Ok(GrpcResponse::new(ExecuteSqlResponse {
                status: Some(ok_response("ok")),
                columns: Vec::new(),
                rows: Vec::new(),
                rows_affected: result.rows_affected(),
            }))
        }
    }

    async fn list_tables(
        &self,
        request: Request<ListTablesRequest>,
    ) -> Result<GrpcResponse<ListTablesResponse>, Status> {
        let request = request.into_inner();
        let pool = connect_any(&request.dsn).await?;
        let sql = match engine_from_dsn(&request.dsn)? {
            DatabaseEngineKind::Mysql => "SHOW TABLES",
            DatabaseEngineKind::Postgres => {
                "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename"
            }
            DatabaseEngineKind::Sqlite => {
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
            }
        };
        let rows = sqlx::query(sql).fetch_all(&pool).await.map_err(db_status)?;
        let tables = rows
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>(0).ok())
            .collect::<Vec<_>>();
        Ok(GrpcResponse::new(ListTablesResponse {
            status: Some(ok_response("ok")),
            tables,
        }))
    }

    async fn browse_table(
        &self,
        request: Request<BrowseTableRequest>,
    ) -> Result<GrpcResponse<BrowseTableResponse>, Status> {
        let request = request.into_inner();
        // 表名严格校验(字母数字 + 下划线)后再带引号拼入 SQL,杜绝注入。
        validate_identifier(&request.table)?;
        let engine = engine_from_dsn(&request.dsn)?;
        let quoted = quote_identifier(engine, &request.table);
        let limit = request.limit.clamp(1, 500);
        let offset = request.offset;
        let pool = connect_any(&request.dsn).await?;

        let count_sql = format!("SELECT COUNT(*) FROM {quoted}");
        let total_rows = sqlx::query(&count_sql)
            .fetch_one(&pool)
            .await
            .ok()
            .and_then(|row| row.try_get::<i64, _>(0).ok())
            .map(|count| count.max(0) as u64)
            .unwrap_or(0);

        let data_sql = format!("SELECT * FROM {quoted} LIMIT {limit} OFFSET {offset}");
        let rows = sqlx::query(&data_sql)
            .fetch_all(&pool)
            .await
            .map_err(db_status)?;
        let columns = rows
            .first()
            .map(|row| {
                row.columns()
                    .iter()
                    .map(|column| column.name().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let rows = rows
            .into_iter()
            .map(|row| SqlRow {
                values: (0..row.len())
                    .map(|index| value_to_string(&row, index))
                    .collect(),
            })
            .collect::<Vec<_>>();

        Ok(GrpcResponse::new(BrowseTableResponse {
            status: Some(ok_response("ok")),
            columns,
            rows,
            total_rows,
        }))
    }

    async fn import_sql(
        &self,
        request: Request<ImportSqlRequest>,
    ) -> Result<GrpcResponse<ImportSqlResponse>, Status> {
        let request = request.into_inner();
        let pool = connect_any(&request.dsn).await?;
        // 朴素按 ; 拆句逐条执行(v1):适合常规 dump;字符串/存储过程内的 ; 不支持,
        // 前端会提示"复杂脚本请用 SQL 控制台"。
        let mut executed = 0u32;
        for statement in split_sql_statements(&request.sql) {
            sqlx::query(&statement)
                .execute(&pool)
                .await
                .map_err(db_status)?;
            executed += 1;
        }
        Ok(GrpcResponse::new(ImportSqlResponse {
            status: Some(ok_response("ok")),
            statements_executed: executed,
        }))
    }

    async fn database_overview(
        &self,
        request: Request<DatabaseOverviewRequest>,
    ) -> Result<GrpcResponse<DatabaseOverviewResponse>, Status> {
        let request = request.into_inner();
        let engine = engine_from_dsn(&request.dsn)?;
        let pool = connect_any(&request.dsn).await?;
        let (version_sql, conn_sql, uptime_sql) = match engine {
            DatabaseEngineKind::Mysql => (
                "SELECT VERSION()",
                "SELECT COUNT(*) FROM information_schema.processlist",
                "SELECT VARIABLE_VALUE FROM performance_schema.global_status WHERE VARIABLE_NAME = 'Uptime'",
            ),
            DatabaseEngineKind::Postgres => (
                "SELECT version()",
                "SELECT count(*) FROM pg_stat_activity",
                "SELECT EXTRACT(EPOCH FROM (now() - pg_postmaster_start_time()))::bigint",
            ),
            DatabaseEngineKind::Sqlite => ("SELECT sqlite_version()", "SELECT 1", "SELECT 0"),
        };
        let version = sqlx::query(version_sql)
            .fetch_one(&pool)
            .await
            .ok()
            .and_then(|row| row.try_get::<String, _>(0).ok())
            .unwrap_or_default();
        let active_connections = fetch_scalar_u64(&pool, conn_sql).await;
        let uptime_seconds = fetch_scalar_u64(&pool, uptime_sql).await;
        Ok(GrpcResponse::new(DatabaseOverviewResponse {
            status: Some(ok_response("ok")),
            version,
            active_connections,
            uptime_seconds,
        }))
    }

    async fn backup_database(
        &self,
        request: Request<BackupDatabaseRequest>,
    ) -> Result<GrpcResponse<BackupDatabaseResponse>, Status> {
        let request = request.into_inner();
        validate_identifier(&request.database)?;
        let engine = engine_from_dsn(&request.dsn)?;
        let download_url = match engine {
            DatabaseEngineKind::Mysql | DatabaseEngineKind::Postgres => {
                format!("/api/db/backup?database={}", request.database)
            }
            DatabaseEngineKind::Sqlite => "/api/db/backup/sqlite".to_owned(),
        };

        Ok(GrpcResponse::new(BackupDatabaseResponse {
            status: Some(ok_response("backup stream prepared")),
            download_url,
        }))
    }

    // ====== Phase D: 轻量数据库优先 ======

    async fn list_sqlite_files(
        &self,
        request: Request<ListSqliteFilesRequest>,
    ) -> Result<GrpcResponse<ListSqliteFilesResponse>, Status> {
        let req = request.into_inner();
        let mut roots: Vec<PathBuf> = if req.scan_dirs.is_empty() {
            sqlite_default_roots()
        } else {
            req.scan_dirs.into_iter().map(PathBuf::from).collect()
        };
        roots.sort();
        roots.dedup();

        let mut files = Vec::new();
        for root in roots {
            collect_sqlite_files(&root, &mut files).await;
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(GrpcResponse::new(ListSqliteFilesResponse {
            status: Some(ok_response("ok")),
            files,
        }))
    }

    async fn create_sqlite_file(
        &self,
        request: Request<CreateSqliteFileRequest>,
    ) -> Result<GrpcResponse<CreateSqliteFileResponse>, Status> {
        let req = request.into_inner();
        let path = PathBuf::from(req.path.trim());
        if path.as_os_str().is_empty() {
            return Err(Status::invalid_argument("path is required"));
        }
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_status)?;
        }
        let dsn = format!("sqlite:{}?mode=rwc", path.display());
        SQLX_DRIVERS.call_once(sqlx::any::install_default_drivers);
        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .map_err(db_status)?;
        // 跑一句 PRAGMA 以确保文件落盘
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await
            .map_err(db_status)?;
        let metadata = tokio::fs::metadata(&path).await.map_err(io_status)?;
        let modified_at_seconds = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Ok(GrpcResponse::new(CreateSqliteFileResponse {
            status: Some(ok_response("created")),
            file: Some(SqliteFile {
                path: path.to_string_lossy().into_owned(),
                size_bytes: metadata.len(),
                modified_at_seconds,
            }),
        }))
    }

    async fn vacuum_sqlite(
        &self,
        request: Request<VacuumSqliteRequest>,
    ) -> Result<GrpcResponse<VacuumSqliteResponse>, Status> {
        let req = request.into_inner();
        let path = PathBuf::from(req.path.trim());
        let before = tokio::fs::metadata(&path).await.map_err(io_status)?.len();
        let dsn = format!("sqlite:{}", path.display());
        SQLX_DRIVERS.call_once(sqlx::any::install_default_drivers);
        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .map_err(db_status)?;
        sqlx::query("VACUUM")
            .execute(&pool)
            .await
            .map_err(db_status)?;
        let after = tokio::fs::metadata(&path).await.map_err(io_status)?.len();
        Ok(GrpcResponse::new(VacuumSqliteResponse {
            status: Some(ok_response("vacuumed")),
            size_before_bytes: before,
            size_after_bytes: after,
        }))
    }

    async fn get_redis_info(
        &self,
        request: Request<GetRedisInfoRequest>,
    ) -> Result<GrpcResponse<GetRedisInfoResponse>, Status> {
        let url = {
            let raw = request.into_inner().url;
            if raw.trim().is_empty() {
                "redis://127.0.0.1:6379".to_owned()
            } else {
                raw
            }
        };
        let info = match probe_redis_info(&url).await {
            Ok(value) => value,
            Err(err) => RedisInfo {
                reachable: false,
                error: err.to_string(),
                ..Default::default()
            },
        };
        Ok(GrpcResponse::new(GetRedisInfoResponse {
            status: Some(ok_response("ok")),
            info: Some(info),
        }))
    }
}

fn sqlite_default_roots() -> Vec<PathBuf> {
    if let Ok(value) = env::var("RUSTPANEL_SQLITE_ROOTS") {
        return value
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    DEFAULT_SQLITE_ROOTS.iter().map(PathBuf::from).collect()
}

// 异步递归扫一层目录,识别 SQLite 文件(magic bytes "SQLite format 3\0")
async fn collect_sqlite_files(root: &Path, files: &mut Vec<SqliteFile>) {
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let meta = match entry.metadata().await {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            // 仅打开前 16 字节验证 SQLite 头
            if !is_sqlite_file(&path).await {
                continue;
            }
            let modified_at_seconds = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            files.push(SqliteFile {
                path: path.to_string_lossy().into_owned(),
                size_bytes: meta.len(),
                modified_at_seconds,
            });
        }
    }
}

async fn is_sqlite_file(path: &Path) -> bool {
    use tokio::io::AsyncReadExt;
    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 16];
    if file.read_exact(&mut buf).await.is_err() {
        return false;
    }
    &buf == b"SQLite format 3\x00"
}

async fn probe_redis_info(url: &str) -> Result<RedisInfo, redis::RedisError> {
    let client = redis::Client::open(url)?;
    let mut conn = client.get_multiplexed_async_connection().await?;
    let raw: String = redis::cmd("INFO").query_async(&mut conn).await?;
    let mut info = RedisInfo {
        reachable: true,
        ..Default::default()
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(':') {
            Some(pair) => pair,
            None => continue,
        };
        match key {
            "redis_version" => info.version = value.to_owned(),
            "redis_mode" => info.mode = value.to_owned(),
            "connected_clients" => info.connected_clients = value.parse().unwrap_or(0),
            "used_memory" => info.used_memory_bytes = value.parse().unwrap_or(0),
            "maxmemory" => info.max_memory_bytes = value.parse().unwrap_or(0),
            "maxmemory_policy" => info.max_memory_policy = value.to_owned(),
            "keyspace_hits" => info.keyspace_hits = value.parse().unwrap_or(0),
            "keyspace_misses" => info.keyspace_misses = value.parse().unwrap_or(0),
            "total_commands_processed" => {
                info.total_commands_processed = value.parse().unwrap_or(0)
            }
            "uptime_in_seconds" => info.uptime_seconds = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    Ok(info)
}

fn io_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

async fn connect_any(dsn: &str) -> Result<sqlx::AnyPool, Status> {
    SQLX_DRIVERS.call_once(sqlx::any::install_default_drivers);
    AnyPoolOptions::new()
        .max_connections(5)
        .connect(dsn)
        .await
        .map_err(db_status)
}

fn engine_from_dsn(dsn: &str) -> Result<DatabaseEngineKind, Status> {
    if dsn.starts_with("mysql://") {
        Ok(DatabaseEngineKind::Mysql)
    } else if dsn.starts_with("postgres://") || dsn.starts_with("postgresql://") {
        Ok(DatabaseEngineKind::Postgres)
    } else if dsn.starts_with("sqlite:") {
        Ok(DatabaseEngineKind::Sqlite)
    } else {
        Err(Status::invalid_argument(
            "DSN must start with mysql://, postgres://, postgresql://, or sqlite:",
        ))
    }
}

/// 朴素 SQL 拆句:按 ; 分;每段逐行剔掉空行与整行 `--` 注释后再拼回。
/// v1,不处理字符串字面量内部的 ;(前端提示复杂脚本用 SQL 控制台)。
fn split_sql_statements(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(|segment| {
            segment
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with("--"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|stmt| !stmt.is_empty())
        .collect()
}

/// 取单标量并尽量转成 u64(i64 / f64 / 字符串数字都兼容),失败返 0。
async fn fetch_scalar_u64(pool: &sqlx::AnyPool, sql: &str) -> u64 {
    match sqlx::query(sql).fetch_one(pool).await {
        Ok(row) => row
            .try_get::<i64, _>(0)
            .map(|value| value.max(0) as u64)
            .or_else(|_| row.try_get::<f64, _>(0).map(|value| value.max(0.0) as u64))
            .or_else(|_| {
                row.try_get::<String, _>(0)
                    .map(|value| value.trim().parse::<u64>().unwrap_or(0))
            })
            .unwrap_or(0),
        Err(_) => 0,
    }
}

/// 给已 validate_identifier 校验过的标识符加引擎对应的引号。
fn quote_identifier(engine: DatabaseEngineKind, ident: &str) -> String {
    match engine {
        DatabaseEngineKind::Mysql => format!("`{ident}`"),
        DatabaseEngineKind::Postgres | DatabaseEngineKind::Sqlite => format!("\"{ident}\""),
    }
}

fn validate_identifier(identifier: &str) -> Result<(), Status> {
    let valid = !identifier.is_empty()
        && identifier
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '_');
    if valid {
        Ok(())
    } else {
        Err(Status::invalid_argument("invalid SQL identifier"))
    }
}

fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn returns_rows(sql: &str) -> bool {
    let sql = sql.trim_start().to_ascii_lowercase();
    sql.starts_with("select")
        || sql.starts_with("show")
        || sql.starts_with("with")
        || sql.starts_with("pragma")
}

fn value_to_string(row: &sqlx::any::AnyRow, index: usize) -> String {
    row.try_get::<String, _>(index)
        .or_else(|_| row.try_get::<i64, _>(index).map(|value| value.to_string()))
        .or_else(|_| row.try_get::<f64, _>(index).map(|value| value.to_string()))
        .or_else(|_| row.try_get::<bool, _>(index).map(|value| value.to_string()))
        .unwrap_or_else(|_| String::new())
}

fn db_status(error: impl std::fmt::Display) -> Status {
    Status::internal(error.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DatabaseEngineKind {
    Mysql,
    Postgres,
    Sqlite,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_identifier() {
        assert!(validate_identifier("valid_name_1").is_ok());
        assert!(validate_identifier("bad-name").is_err());
        assert!(validate_identifier("name;drop").is_err());
    }

    #[test]
    fn detects_query_shape() {
        assert!(returns_rows("SELECT 1"));
        assert!(!returns_rows("UPDATE users SET name = 'a'"));
    }

    #[test]
    fn splits_sql_statements_and_skips_blanks_and_comments() {
        let stmts = split_sql_statements(
            "CREATE TABLE t(id int);\n-- a comment\nINSERT INTO t VALUES (1);\n\n",
        );
        assert_eq!(stmts.len(), 2);
        assert_eq!(stmts[0], "CREATE TABLE t(id int)");
        assert_eq!(stmts[1], "INSERT INTO t VALUES (1)");
    }

    #[test]
    fn quotes_identifier_per_engine() {
        assert_eq!(quote_identifier(DatabaseEngineKind::Mysql, "t"), "`t`");
        assert_eq!(quote_identifier(DatabaseEngineKind::Postgres, "t"), "\"t\"");
    }
}
