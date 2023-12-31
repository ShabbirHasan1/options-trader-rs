use serde::Deserialize;
use std::fs::File;
use std::io::prelude::*;

use std::collections::HashMap;

use anyhow::Result;

#[derive(Default, Clone, Debug, Deserialize)]
pub struct Settings {
    pub database: DatabaseConfig,
}

#[derive(Default, Clone, Debug, Deserialize)]
pub struct DatabaseConfig {
    pub name: String,
    pub port: u16,
    pub host: String,
    pub user: String,
}

#[derive(Debug)]
pub struct Config {}

impl Config {
    pub fn read_config_file(path: &str) -> Result<Settings> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let settings: Settings = serde_json::from_str(&contents)?;
        Ok(settings)
    }
}
