use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_sec: u64,
    pub idle_timeout_sec: u64,
    pub statement_timeout_ms: u64,
    pub lock_timeout_ms: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "postgres://athena@localhost:5432/athenadb".into(),
            max_connections: 50,
            min_connections: 1,
            connect_timeout_sec: 10,
            idle_timeout_sec: 300,
            statement_timeout_ms: 15_000,
            lock_timeout_ms: 5_000,
        }
    }
}

impl DatabaseConfig {
    pub fn validate_url(&self) -> Result<(), String> {
        self.url
            .parse::<sqlx::postgres::PgConnectOptions>()
            .map(|_| ())
            .map_err(|_| "Invalid PostgreSQL connection URL".into())
    }

    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_sec)
    }

    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.idle_timeout_sec)
    }
}
