use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::db_client::SqlQueryBuilder;


pub struct MockDb {
    pub pool: HashMap<String, HashMap<String, String>>,
}

impl MockDb {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            pool: HashMap::new(),
        })
    }

    pub fn get_sql_stmt(
        &self,
        table_name: &str,
        local_id: &Uuid,
        columns: Vec<&str>,
        _db: &Arc<MockDb>,
    ) -> String {
        if Uuid::is_nil(local_id) && !table_name.eq("tasty_auth") {
            SqlQueryBuilder::prepare_insert_statement(table_name, &columns)
        } else {
            SqlQueryBuilder::prepare_update_statement(table_name, &columns)
        }
    }
}
