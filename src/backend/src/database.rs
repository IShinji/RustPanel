use std::sync::Once;

use sqlx::{any::AnyPoolOptions, Column, Row};
use tonic::{Request, Response as GrpcResponse, Status};

use crate::{
    ok_response,
    proto::rustpanel::v1::{
        database_service_server::DatabaseService, BackupDatabaseRequest, BackupDatabaseResponse,
        CreateDatabaseRequest, CreateDatabaseResponse, CreateDatabaseUserRequest,
        CreateDatabaseUserResponse, DatabaseItem, ExecuteSqlRequest, ExecuteSqlResponse,
        ListDatabasesRequest, ListDatabasesResponse, SqlRow,
    },
};

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
}
