use anyhow::bail;
use anyhow::Ok;
use anyhow::Result;
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use sqlx::Pool;
use sqlx::Postgres;
use std::env;
use tracing::info;

use super::settings::Settings;

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub name: String,
    pub port: u16,
    pub host: String,
    pub user: String,
}

pub async fn startup_db(settings: &Settings) -> DBClient {
    match DBClient::new(settings).await {
        Err(val) => {
            info!("Settings file error: {val}");
            std::process::exit(1);
        }
        anyhow::Result::Ok(val) => val,
    }
}
#[derive(Debug)]
pub struct SqlQueryBuilder;

impl SqlQueryBuilder {
    pub fn prepare_insert_statement(table: &str, columns: &[&str]) -> String {
        let sql = format!("INSERT INTO {} ({})", table, columns.join(", "));
        let placeholders: String = (1..=columns.len())
            .map(|i| format!("${}", i))
            .collect::<Vec<String>>()
            .join(", ");

        format!("{} VALUES ({})", sql, placeholders)
    }

    pub fn prepare_update_statement(table: &str, columns: &[&str]) -> String {
        let sql = format!("UPDATE {} SET", table);

        let placeholders: String = (1..=columns.len() - 1)
            .map(|i| format!("{} = ${}", columns[i - 1], i))
            .collect::<Vec<String>>()
            .join(", ");

        let num_of_cols = columns.len();
        format!(
            "{} {} WHERE {} = ${}",
            sql,
            placeholders,
            columns[num_of_cols - 1],
            num_of_cols
        )
    }

    pub fn prepare_fetch_statement(table: &str, filters: &[&str]) -> String {
        if filters.is_empty() {
            return format!("SELECT * FROM {}", table);
        }

        let sql = format!("SELECT * FROM {}", table);
        let placeholders: String = (1..=filters.len())
            .map(|i| format!("{} = ${}", filters[i - 1], i))
            .collect::<Vec<String>>()
            .join(" AND ");

        let sql = format!("{} WHERE {}", sql, placeholders);
        sql
    }

    #[cfg(test)]
    pub fn prepare_delete_statement(table: &str, columns: &[&str]) -> String {
        if columns.is_empty() {
            return format!("DELETE FROM {}", table);
        }

        let sql = format!("DELETE FROM {}", table);
        let placeholders: String = (1..=columns.len())
            .map(|i| format!("{} = ${}", columns[i - 1], i))
            .collect::<Vec<String>>()
            .join(" AND ");

        format!("{} WHERE {}", sql, placeholders)
    }
}

#[derive(Debug)]
pub struct DBClient {
    pub pool: Pool<Postgres>,
}

impl DBClient {
    pub async fn new(settings: &Settings) -> Result<Self> {
        let db_cfg = &settings.database;
        let dbpass =
            env::var("DB_PASSWORD").expect("Failed to read the 'dbpass' environment variable.");
        let database_url = format!(
            "postgresql://{}:{}@{}:{}/{}?sslmode=disable",
            db_cfg.user, dbpass, db_cfg.host, db_cfg.port, db_cfg.name
        );
        let pool = match PgPoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .test_before_acquire(false)
            .connect(&database_url)
            .await
        {
            std::result::Result::Ok(pool) => pool,
            std::result::Result::Err(err) => {
                bail!(
                    "Failed to startup db connection pool with url: {} error={}",
                    database_url,
                    err
                );
            }
        };

        Ok(Self { pool })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_insert_statement() {
        let _builder = SqlQueryBuilder {};

        let table = "test";
        let columns = vec!["one", "two", "three", "four"];
        let sql = SqlQueryBuilder::prepare_insert_statement(table, &columns);
        assert_eq!(
            sql,
            "INSERT INTO test (one, two, three, four) VALUES ($1, $2, $3, $4)"
        );
    }

    #[test]
    fn test_sql_update_statement() {
        let table = "test";
        let columns = vec!["one", "two", "three", "four", "local_id"];
        let sql = SqlQueryBuilder::prepare_update_statement(table, &columns);
        assert_eq!(
            sql,
            "UPDATE test SET one = $1, two = $2, three = $3, four = $4 WHERE local_id = $5"
        );
    }

    #[test]
    fn test_sql_fetch_statement_whole_table() {
        let table = "test";
        let sql = SqlQueryBuilder::prepare_fetch_statement(table, &Vec::default());
        assert_eq!(sql, "SELECT * FROM test");
    }

    #[test]
    fn test_sql_fetch_statement_with_filter() {
        let table = "test";
        let columns = vec!["one", "two", "three"];
        let sql = SqlQueryBuilder::prepare_fetch_statement(table, &columns);
        assert_eq!(
            sql,
            "SELECT * FROM test WHERE one = $1 AND two = $2 AND three = $3"
        );
    }

    #[test]
    fn test_sql_delete_statement() {
        let table = "test";
        let sql = SqlQueryBuilder::prepare_delete_statement(table, &Vec::default());
        assert_eq!(sql, "DELETE FROM test");
    }

    #[test]
    fn test_sql_delete_statement_with_filters() {
        let table = "test";
        let columns = vec!["one", "two", "three"];
        let sql = SqlQueryBuilder::prepare_delete_statement(table, &columns);
        assert_eq!(
            sql,
            "DELETE FROM test WHERE one = $1 AND two = $2 AND three = $3"
        );
    }
}
